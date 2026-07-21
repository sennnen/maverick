import CoreBluetooth
import Foundation

@MainActor
final class MavBluetoothExecutor: NSObject {
  private static let restorationKey = "connector.ble.restoration.v1"
  private static let restorationIdentifier = "com.sennnen.mav.connector-central"

  var checkpoint: ConnectorRestorationCheckpoint? {
    didSet { persistCheckpoint() }
  }

  private let eventSink: (ConnectorTransportEvent) -> Void
  private lazy var central = CBCentralManager(
    delegate: self,
    queue: nil,
    options: [CBCentralManagerOptionRestoreIdentifierKey: Self.restorationIdentifier]
  )
  private var queued: [ConnectorTransportAction] = []
  private var peripherals: [String: CBPeripheral] = [:]
  private var peripheral: CBPeripheral?
  private var manufacturerFilter = Set<UInt16>()
  private var timers: [UInt64: DispatchWorkItem] = [:]
  private var pendingCharacteristicDiscovery = 0
  private var logicalIDs: [String: String] = [:]
  private var pendingReads: [String: (operationID: UInt64, logicalID: String)] = [:]
  private var pendingWrites: [String: (operationID: UInt64, logicalID: String)] = [:]

  init(eventSink: @escaping (ConnectorTransportEvent) -> Void) {
    self.eventSink = eventSink
    super.init()
    restoreCheckpoint()
    _ = central
  }

  func execute(_ action: ConnectorTransportAction) {
    guard isCurrent(action) else { return }
    guard central.state == .poweredOn else {
      queued.append(action)
      return
    }
    perform(action)
  }

  private func perform(_ action: ConnectorTransportAction) {
    switch ConnectorNativeOperation.map(action.request) {
    case let .scan(serviceUUIDs, manufacturerIDs):
      manufacturerFilter = Set(manufacturerIDs)
      let services = serviceUUIDs.isEmpty ? nil : serviceUUIDs.map(CBUUID.init(string:))
      central.scanForPeripherals(
        withServices: services,
        options: [CBCentralManagerScanOptionAllowDuplicatesKey: false]
      )
    case .stopScan:
      central.stopScan()
      eventSink(.scanStopped(reasonCode: 0))
    case let .connect(address):
      guard let target = peripherals[address] else {
        fail(action.operationId, code: 1, "The selected Bluetooth device is no longer visible.")
        return
      }
      peripheral = target
      target.delegate = self
      central.connect(target, options: nil)
    case .ensurePaired:
      // CoreBluetooth owns pairing and presents system UI on the first protected operation.
      eventSink(.pairingResult(success: true, errorCode: nil))
    case .discoverServices:
      guard let peripheral else {
        fail(action.operationId, code: 2, "No Bluetooth device is connected.")
        return
      }
      peripheral.discoverServices(nil)
    case let .subscribe(id, service, characteristic):
      guard let native = findCharacteristic(service: service, characteristic: characteristic) else {
        fail(action.operationId, code: 3, "A declared Bluetooth characteristic was not discovered.")
        return
      }
      logicalIDs[key(native)] = id
      peripheral?.setNotifyValue(true, for: native)
    case let .unsubscribe(id, service, characteristic):
      guard let native = findCharacteristic(service: service, characteristic: characteristic) else {
        fail(action.operationId, code: 3, "A declared Bluetooth characteristic was not discovered.")
        return
      }
      logicalIDs[key(native)] = id
      peripheral?.setNotifyValue(false, for: native)
    case let .read(id, service, characteristic):
      guard let native = findCharacteristic(service: service, characteristic: characteristic) else {
        fail(action.operationId, code: 3, "A declared Bluetooth characteristic was not discovered.")
        return
      }
      pendingReads[key(native)] = (action.operationId, id)
      peripheral?.readValue(for: native)
    case let .write(id, service, characteristic, bytes, confirmed):
      guard let native = findCharacteristic(service: service, characteristic: characteristic) else {
        fail(action.operationId, code: 3, "A declared Bluetooth characteristic was not discovered.")
        return
      }
      let type: CBCharacteristicWriteType = confirmed ? .withResponse : .withoutResponse
      if confirmed { pendingWrites[key(native)] = (action.operationId, id) }
      peripheral?.writeValue(bytes, for: native, type: type)
      if !confirmed {
        eventSink(.writeResult(operationId: action.operationId, characteristicId: id))
      }
    case .disconnect:
      if let peripheral { central.cancelPeripheralConnection(peripheral) }
    case let .setTimer(token, delayMs):
      timers[token]?.cancel()
      let item = DispatchWorkItem { [weak self] in
        self?.timers[token] = nil
        self?.eventSink(.timerFired(token: token))
      }
      timers[token] = item
      DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(Int(clamping: delayMs)), execute: item)
    case let .cancelTimer(token):
      timers.removeValue(forKey: token)?.cancel()
    }
  }

  private func isCurrent(_ action: ConnectorTransportAction) -> Bool {
    guard let checkpoint else { return false }
    return checkpoint.connectorID == action.connectorId
      && checkpoint.sessionID == action.sessionId
      && checkpoint.cancellationGeneration == action.cancellationGeneration
  }

  private func findCharacteristic(service: String, characteristic: String) -> CBCharacteristic? {
    let serviceUUID = CBUUID(string: service)
    let characteristicUUID = CBUUID(string: characteristic)
    return peripheral?.services?
      .first(where: { $0.uuid == serviceUUID })?
      .characteristics?
      .first(where: { $0.uuid == characteristicUUID })
  }

  private func key(_ characteristic: CBCharacteristic) -> String {
    "\(characteristic.service?.uuid.uuidString.lowercased() ?? "")|\(characteristic.uuid.uuidString.lowercased())"
  }

  private func fail(_ operationID: UInt64?, code: UInt16, _ message: String) {
    eventSink(.transportError(operationId: operationID, code: code, safeMessage: message))
  }

  private func persistCheckpoint() {
    guard let checkpoint, let data = try? JSONEncoder().encode(checkpoint) else {
      UserDefaults.standard.removeObject(forKey: Self.restorationKey)
      return
    }
    UserDefaults.standard.set(data, forKey: Self.restorationKey)
  }

  private func restoreCheckpoint() {
    guard
      let data = UserDefaults.standard.data(forKey: Self.restorationKey),
      let restored = try? JSONDecoder().decode(ConnectorRestorationCheckpoint.self, from: data)
    else { return }
    checkpoint = restored
  }
}

extension MavBluetoothExecutor: CBCentralManagerDelegate {
  func centralManagerDidUpdateState(_ central: CBCentralManager) {
    guard central.state == .poweredOn else { return }
    let pending = queued
    queued.removeAll(keepingCapacity: true)
    pending.forEach(perform)
  }

  func centralManager(
    _ central: CBCentralManager,
    willRestoreState dict: [String: Any]
  ) {
    let restored = dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral] ?? []
    for item in restored {
      peripherals[item.identifier.uuidString] = item
      item.delegate = self
      if item.state == .connected { peripheral = item }
    }
  }

  func centralManager(
    _ central: CBCentralManager,
    didDiscover peripheral: CBPeripheral,
    advertisementData: [String: Any],
    rssi RSSI: NSNumber
  ) {
    let manufacturerData = advertisementData[CBAdvertisementDataManufacturerDataKey] as? Data ?? Data()
    if !manufacturerFilter.isEmpty {
      guard manufacturerData.count >= 2 else { return }
      let identifier = UInt16(manufacturerData[0]) | (UInt16(manufacturerData[1]) << 8)
      guard manufacturerFilter.contains(identifier) else { return }
    }
    let address = peripheral.identifier.uuidString
    peripherals[address] = peripheral
    let services = (advertisementData[CBAdvertisementDataServiceUUIDsKey] as? [CBUUID] ?? [])
      .map(\.uuidString)
    eventSink(.advertisement(
      address: address,
      rssi: Int16(clamping: RSSI.intValue),
      serviceUuids: services,
      manufacturerData: manufacturerData,
      name: advertisementData[CBAdvertisementDataLocalNameKey] as? String ?? peripheral.name
    ))
  }

  func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
    self.peripheral = peripheral
    peripheral.delegate = self
    let mtu = min(peripheral.maximumWriteValueLength(for: .withoutResponse) + 3, Int(UInt16.max))
    eventSink(.connected(mtu: UInt16(mtu)))
  }

  func centralManager(
    _ central: CBCentralManager,
    didFailToConnect peripheral: CBPeripheral,
    error: Error?
  ) {
    fail(nil, code: 4, error?.localizedDescription ?? "Bluetooth connection failed.")
  }

  func centralManager(
    _ central: CBCentralManager,
    didDisconnectPeripheral peripheral: CBPeripheral,
    error: Error?
  ) {
    self.peripheral = nil
    eventSink(.disconnected(reasonCode: error == nil ? 0 : 1))
  }
}

extension MavBluetoothExecutor: CBPeripheralDelegate {
  func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
    if let error {
      fail(nil, code: 5, error.localizedDescription)
      return
    }
    let services = peripheral.services ?? []
    pendingCharacteristicDiscovery = services.count
    if services.isEmpty {
      eventSink(.servicesDiscovered(serviceUuids: []))
    } else {
      services.forEach { peripheral.discoverCharacteristics(nil, for: $0) }
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didDiscoverCharacteristicsFor service: CBService,
    error: Error?
  ) {
    if let error {
      fail(nil, code: 6, error.localizedDescription)
      return
    }
    pendingCharacteristicDiscovery = max(0, pendingCharacteristicDiscovery - 1)
    if pendingCharacteristicDiscovery == 0 {
      eventSink(.servicesDiscovered(serviceUuids: (peripheral.services ?? []).map { $0.uuid.uuidString }))
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didUpdateNotificationStateFor characteristic: CBCharacteristic,
    error: Error?
  ) {
    let logicalID = logicalIDs[key(characteristic)] ?? characteristic.uuid.uuidString
    if let error {
      fail(nil, code: 7, error.localizedDescription)
    } else if characteristic.isNotifying {
      eventSink(.subscribed(characteristicId: logicalID))
    } else {
      eventSink(.unsubscribed(characteristicId: logicalID))
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didUpdateValueFor characteristic: CBCharacteristic,
    error: Error?
  ) {
    let nativeKey = key(characteristic)
    let bytes = characteristic.value ?? Data()
    if let pending = pendingReads.removeValue(forKey: nativeKey) {
      if let error {
        fail(pending.operationID, code: 8, error.localizedDescription)
      } else {
        eventSink(.readResult(
          operationId: pending.operationID,
          characteristicId: pending.logicalID,
          bytes: bytes
        ))
      }
      return
    }
    if let error {
      fail(nil, code: 8, error.localizedDescription)
      return
    }
    let logicalID = logicalIDs[nativeKey] ?? characteristic.uuid.uuidString
    eventSink(.notification(characteristicId: logicalID, bytes: bytes))
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didWriteValueFor characteristic: CBCharacteristic,
    error: Error?
  ) {
    guard let pending = pendingWrites.removeValue(forKey: key(characteristic)) else { return }
    if let error {
      fail(pending.operationID, code: 9, error.localizedDescription)
    } else {
      eventSink(.writeResult(
        operationId: pending.operationID,
        characteristicId: pending.logicalID
      ))
    }
  }
}
