package com.sennnen.mav.connector

import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import androidx.core.content.ContextCompat
import java.util.UUID
import uniffi.mav_ffi.ConnectorTransportAction
import uniffi.mav_ffi.ConnectorTransportEvent

class MavBleExecutor(
    context: Context,
    private val eventSink: (ConnectorTransportEvent) -> Unit,
    private val discoverySink: (List<ConnectorScanDevice>) -> Unit,
    private val errorSink: (String) -> Unit,
) : AutoCloseable {
    companion object {
        private const val CHECKPOINT_KEY = "connector.ble.restoration.v1"
        private const val PREFS = "connector_transport"
        private const val BASE_UUID_SUFFIX = "-0000-1000-8000-00805f9b34fb"
        private val CLIENT_CONFIG = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    }

    private val appContext = context.applicationContext
    private val adapter: BluetoothAdapter? =
        appContext.getSystemService(BluetoothManager::class.java)?.adapter
    private val handler = Handler(Looper.getMainLooper())
    private val timers = mutableMapOf<ULong, Runnable>()
    private val devices = mutableMapOf<String, BluetoothDevice>()
    private val logicalIds = mutableMapOf<String, String>()
    private val pendingReads = mutableMapOf<String, Pair<ULong, String>>()
    private val pendingWrites = mutableMapOf<String, Pair<ULong, String>>()
    private val notificationTargets = mutableMapOf<String, Pair<String, Boolean>>()
    private val subscriptionQueue = ArrayDeque<Triple<ConnectorTransportAction, ConnectorNativeOperation, Boolean>>()
    private val queuedForPermission = mutableListOf<ConnectorTransportAction>()
    private val scanCatalog = ConnectorScanCatalog()
    private val advertisements = mutableMapOf<String, ConnectorTransportEvent.Advertisement>()
    private var manufacturerFilter = emptySet<UShort>()
    private var gatt: BluetoothGatt? = null

    var checkpoint: ConnectorRestorationCheckpoint? = restoreCheckpoint()
        set(value) {
            field = value
            val preferences = appContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            if (value == null) preferences.edit().remove(CHECKPOINT_KEY).apply()
            else preferences.edit().putString(CHECKPOINT_KEY, value.encode()).apply()
        }

    private val bondReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != BluetoothDevice.ACTION_BOND_STATE_CHANGED) return
            val state = intent.getIntExtra(BluetoothDevice.EXTRA_BOND_STATE, BluetoothDevice.ERROR)
            when (state) {
                BluetoothDevice.BOND_BONDED -> eventSink(ConnectorTransportEvent.PairingResult(true, null))
                BluetoothDevice.BOND_NONE -> eventSink(ConnectorTransportEvent.PairingResult(false, 1u))
            }
        }
    }

    init {
        ContextCompat.registerReceiver(
            appContext,
            bondReceiver,
            IntentFilter(BluetoothDevice.ACTION_BOND_STATE_CHANGED),
            ContextCompat.RECEIVER_EXPORTED,
        )
    }

    @SuppressLint("MissingPermission")
    fun execute(action: ConnectorTransportAction) {
        if (!isCurrent(action)) return
        val operation = ConnectorNativeOperation.map(action.request)
        if (needsPermission(operation)) {
            queuedForPermission.removeAll { it.operationId == action.operationId }
            queuedForPermission += action
            return
        }
        when (operation) {
            is ConnectorNativeOperation.Scan -> startScan(action, operation)
            ConnectorNativeOperation.StopScan -> {
                if (hasScanPermission()) adapter?.bluetoothLeScanner?.stopScan(scanCallback)
            }
            is ConnectorNativeOperation.Connect -> {
                if (!hasConnectPermission()) {
                    fail(action.operationId, 1u, "Bluetooth permission is required to connect.")
                    return
                }
                val device = devices[operation.address]
                if (device == null) {
                    fail(action.operationId, 2u, "The selected Bluetooth device is no longer visible.")
                    return
                }
                gatt?.close()
                gatt = device.connectGatt(appContext, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
            }
            ConnectorNativeOperation.EnsurePaired -> ensurePaired(action)
            ConnectorNativeOperation.DiscoverServices -> {
                if (hasConnectPermission() && gatt?.discoverServices() != true) {
                    fail(action.operationId, 3u, "Bluetooth service discovery could not start.")
                }
            }
            is ConnectorNativeOperation.Subscribe -> enqueueSubscription(action, operation, true)
            is ConnectorNativeOperation.Unsubscribe -> enqueueSubscription(action, operation, false)
            is ConnectorNativeOperation.Read -> read(action, operation)
            is ConnectorNativeOperation.Write -> write(action, operation)
            ConnectorNativeOperation.Disconnect -> {
                if (hasScanPermission()) adapter?.bluetoothLeScanner?.stopScan(scanCallback)
                if (hasConnectPermission() && gatt != null) gatt?.disconnect()
                else eventSink(ConnectorTransportEvent.Disconnected(0u))
            }
            is ConnectorNativeOperation.SetTimer -> {
                timers.remove(operation.token)?.let(handler::removeCallbacks)
                val runnable = Runnable {
                    timers.remove(operation.token)
                    eventSink(ConnectorTransportEvent.TimerFired(operation.token))
                }
                timers[operation.token] = runnable
                handler.postDelayed(runnable, operation.delayMs.coerceAtMost(Long.MAX_VALUE.toULong()).toLong())
            }
            is ConnectorNativeOperation.CancelTimer -> {
                timers.remove(operation.token)?.let(handler::removeCallbacks)
            }
        }
    }

    fun onPermissionResult(granted: Boolean) {
        val pending = queuedForPermission.toList()
        queuedForPermission.clear()
        if (granted) pending.forEach(::execute)
        else pending.forEach { fail(it.operationId, 1u, "Bluetooth permission was denied.") }
    }

    fun selectDevice(id: String) {
        advertisements[id]?.let(eventSink)
    }

    @SuppressLint("MissingPermission")
    override fun close() {
        timers.values.forEach(handler::removeCallbacks)
        timers.clear()
        subscriptionQueue.clear()
        subscriptionInFlight = false
        if (hasScanPermission()) adapter?.bluetoothLeScanner?.stopScan(scanCallback)
        if (hasConnectPermission()) {
            gatt?.disconnect()
            gatt?.close()
        }
        gatt = null
        runCatching { appContext.unregisterReceiver(bondReceiver) }
    }

    @SuppressLint("MissingPermission")
    private fun startScan(action: ConnectorTransportAction, operation: ConnectorNativeOperation.Scan) {
        if (!hasScanPermission()) {
            fail(action.operationId, 1u, "Bluetooth scan permission is required.")
            return
        }
        val scanner = adapter?.bluetoothLeScanner
        if (scanner == null) {
            fail(action.operationId, 4u, "Bluetooth Low Energy is unavailable.")
            return
        }
        manufacturerFilter = operation.manufacturerIds.toSet()
        scanCatalog.clear()
        advertisements.clear()
        discoverySink(emptyList())
        val filters = operation.serviceUuids.map { value ->
            ScanFilter.Builder().setServiceUuid(ParcelUuid(uuid(value))).build()
        }
        scanner.startScan(
            filters,
            ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build(),
            scanCallback,
        )
    }

    @SuppressLint("MissingPermission")
    private fun ensurePaired(action: ConnectorTransportAction) {
        if (!hasConnectPermission()) {
            fail(action.operationId, 1u, "Bluetooth permission is required to pair.")
            return
        }
        val device = gatt?.device
        when (device?.bondState) {
            BluetoothDevice.BOND_BONDED -> eventSink(ConnectorTransportEvent.PairingResult(true, null))
            null -> fail(action.operationId, 5u, "No Bluetooth device is connected.")
            else -> if (!device.createBond()) {
                fail(action.operationId, 6u, "Android could not start pairing.")
            }
        }
    }

    @SuppressLint("MissingPermission")
    private fun setSubscription(
        action: ConnectorTransportAction,
        operation: ConnectorNativeOperation,
        enabled: Boolean,
    ) {
        if (!hasConnectPermission()) {
            fail(action.operationId, 1u, "Bluetooth permission is required.")
            finishSubscription()
            return
        }
        val target = when (operation) {
            is ConnectorNativeOperation.Subscribe -> Triple(operation.id, operation.service, operation.characteristic)
            is ConnectorNativeOperation.Unsubscribe -> Triple(operation.id, operation.service, operation.characteristic)
            else -> return
        }
        val characteristic = findCharacteristic(target.second, target.third)
        val activeGatt = gatt
        if (characteristic == null || activeGatt == null) {
            fail(action.operationId, 7u, "A declared Bluetooth characteristic was not discovered.")
            finishSubscription()
            return
        }
        val nativeKey = key(characteristic)
        logicalIds[nativeKey] = target.first
        notificationTargets[nativeKey] = target.first to enabled
        if (!activeGatt.setCharacteristicNotification(characteristic, enabled)) {
            fail(action.operationId, 8u, "Bluetooth notification state could not be changed.")
            finishSubscription()
            return
        }
        val descriptor = characteristic.getDescriptor(CLIENT_CONFIG)
        if (descriptor == null) {
            fail(action.operationId, 9u, "The notification descriptor is missing.")
            finishSubscription()
            return
        }
        val value = when {
            !enabled -> BluetoothGattDescriptor.DISABLE_NOTIFICATION_VALUE
            characteristic.properties and BluetoothGattCharacteristic.PROPERTY_INDICATE != 0 ->
                BluetoothGattDescriptor.ENABLE_INDICATION_VALUE
            else -> BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
        }
        val started = if (Build.VERSION.SDK_INT >= 33) {
            activeGatt.writeDescriptor(descriptor, value) == BluetoothStatusCodes.SUCCESS
        } else {
            @Suppress("DEPRECATION")
            descriptor.value = value
            @Suppress("DEPRECATION")
            activeGatt.writeDescriptor(descriptor)
        }
        if (!started) {
            fail(action.operationId, 10u, "Bluetooth descriptor write could not start.")
            finishSubscription()
        }
    }

    private var subscriptionInFlight = false

    private fun enqueueSubscription(
        action: ConnectorTransportAction,
        operation: ConnectorNativeOperation,
        enabled: Boolean,
    ) {
        subscriptionQueue.addLast(Triple(action, operation, enabled))
        startNextSubscription()
    }

    private fun startNextSubscription() {
        if (subscriptionInFlight) return
        val next = subscriptionQueue.removeFirstOrNull() ?: return
        subscriptionInFlight = true
        setSubscription(next.first, next.second, next.third)
    }

    private fun finishSubscription() {
        subscriptionInFlight = false
        startNextSubscription()
    }

    @SuppressLint("MissingPermission")
    private fun read(action: ConnectorTransportAction, operation: ConnectorNativeOperation.Read) {
        if (!hasConnectPermission()) {
            fail(action.operationId, 1u, "Bluetooth permission is required.")
            return
        }
        val characteristic = findCharacteristic(operation.service, operation.characteristic)
        if (characteristic == null || gatt?.readCharacteristic(characteristic) != true) {
            fail(action.operationId, 11u, "Bluetooth read could not start.")
            return
        }
        pendingReads[key(characteristic)] = action.operationId to operation.id
    }

    @SuppressLint("MissingPermission")
    private fun write(action: ConnectorTransportAction, operation: ConnectorNativeOperation.Write) {
        if (!hasConnectPermission()) {
            fail(action.operationId, 1u, "Bluetooth permission is required.")
            return
        }
        val characteristic = findCharacteristic(operation.service, operation.characteristic)
        val activeGatt = gatt
        if (characteristic == null || activeGatt == null) {
            fail(action.operationId, 12u, "A declared Bluetooth characteristic was not discovered.")
            return
        }
        characteristic.writeType = if (operation.confirmed) {
            BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        } else {
            BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
        }
        val started = if (Build.VERSION.SDK_INT >= 33) {
            activeGatt.writeCharacteristic(
                characteristic,
                operation.bytes,
                characteristic.writeType,
            ) == BluetoothStatusCodes.SUCCESS
        } else {
            @Suppress("DEPRECATION")
            characteristic.value = operation.bytes
            @Suppress("DEPRECATION")
            activeGatt.writeCharacteristic(characteristic)
        }
        if (!started) {
            fail(action.operationId, 13u, "Bluetooth write could not start.")
        } else if (operation.confirmed) {
            pendingWrites[key(characteristic)] = action.operationId to operation.id
        } else {
            eventSink(ConnectorTransportEvent.WriteResult(action.operationId, operation.id))
        }
    }

    private fun findCharacteristic(service: String, characteristic: String): BluetoothGattCharacteristic? =
        gatt?.getService(uuid(service))?.getCharacteristic(uuid(characteristic))

    private fun key(characteristic: BluetoothGattCharacteristic): String =
        "${characteristic.service.uuid}|${characteristic.uuid}".lowercase()

    private fun isCurrent(action: ConnectorTransportAction): Boolean = checkpoint?.let {
        it.connectorId == action.connectorId &&
            it.sessionId == action.sessionId &&
            it.cancellationGeneration == action.cancellationGeneration
    } == true

    private fun fail(operationId: ULong?, code: UShort, message: String) {
        errorSink(message)
        eventSink(ConnectorTransportEvent.TransportError(operationId, code, message))
    }

    private fun hasScanPermission(): Boolean = if (Build.VERSION.SDK_INT >= 31) {
        ContextCompat.checkSelfPermission(appContext, Manifest.permission.BLUETOOTH_SCAN) ==
            PackageManager.PERMISSION_GRANTED
    } else {
        ContextCompat.checkSelfPermission(appContext, Manifest.permission.ACCESS_FINE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED
    }

    private fun hasConnectPermission(): Boolean = Build.VERSION.SDK_INT < 31 ||
        ContextCompat.checkSelfPermission(appContext, Manifest.permission.BLUETOOTH_CONNECT) ==
        PackageManager.PERMISSION_GRANTED

    private fun needsPermission(operation: ConnectorNativeOperation): Boolean = when (operation) {
        is ConnectorNativeOperation.Scan, ConnectorNativeOperation.StopScan -> !hasScanPermission()
        is ConnectorNativeOperation.Connect,
        ConnectorNativeOperation.EnsurePaired,
        ConnectorNativeOperation.DiscoverServices,
        is ConnectorNativeOperation.Subscribe,
        is ConnectorNativeOperation.Unsubscribe,
        is ConnectorNativeOperation.Read,
        is ConnectorNativeOperation.Write,
        ConnectorNativeOperation.Disconnect,
        -> !hasConnectPermission()
        is ConnectorNativeOperation.SetTimer, is ConnectorNativeOperation.CancelTimer -> false
    }

    private fun restoreCheckpoint(): ConnectorRestorationCheckpoint? =
        appContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(CHECKPOINT_KEY, null)
            ?.let(ConnectorRestorationCheckpoint::decode)

    private fun uuid(value: String): UUID {
        val normalized = value.trim().lowercase().removePrefix("0x")
        return when (normalized.length) {
            4 -> UUID.fromString("0000$normalized$BASE_UUID_SUFFIX")
            8 -> UUID.fromString("$normalized$BASE_UUID_SUFFIX")
            else -> UUID.fromString(normalized)
        }
    }

    private val scanCallback = object : ScanCallback() {
        @SuppressLint("MissingPermission")
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val record = result.scanRecord
            val manufacturers = record?.manufacturerSpecificData
            var manufacturerBytes = byteArrayOf()
            if (manufacturers != null && manufacturers.size() > 0) {
                val id = manufacturers.keyAt(0).toUShort()
                if (manufacturerFilter.isNotEmpty() && id !in manufacturerFilter) return
                val body = manufacturers.valueAt(0) ?: byteArrayOf()
                manufacturerBytes = byteArrayOf(
                    (id.toInt() and 0xff).toByte(),
                    ((id.toInt() shr 8) and 0xff).toByte(),
                ) + body
            } else if (manufacturerFilter.isNotEmpty()) {
                return
            }
            devices[result.device.address] = result.device
            val address = result.device.address
            val name = record?.deviceName ?: runCatching { result.device.name }.getOrNull()
            val advertisement = ConnectorTransportEvent.Advertisement(
                address = address,
                rssi = result.rssi.coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt()).toShort(),
                serviceUuids = record?.serviceUuids?.map { connectorWireUuid(it.uuid) } ?: emptyList(),
                manufacturerData = manufacturerBytes,
                name = name,
            )
            advertisements[address] = advertisement
            scanCatalog.observe(
                ConnectorScanDevice(
                    id = address,
                    name = name ?: "Nearby wearable",
                    rssi = result.rssi,
                ),
            )
            discoverySink(scanCatalog.devices())
        }

        override fun onScanFailed(errorCode: Int) {
            eventSink(ConnectorTransportEvent.ScanStopped(errorCode.coerceIn(0, UShort.MAX_VALUE.toInt()).toUShort()))
        }
    }

    private val gattCallback = object : BluetoothGattCallback() {
        @SuppressLint("MissingPermission")
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            if (status != BluetoothGatt.GATT_SUCCESS) {
                eventSink(ConnectorTransportEvent.Disconnected(status.toUShort()))
                gatt.close()
                return
            }
            when (newState) {
                BluetoothProfile.STATE_CONNECTED -> {
                    this@MavBleExecutor.gatt = gatt
                    eventSink(ConnectorTransportEvent.Connected(23u))
                    gatt.requestMtu(517)
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    eventSink(ConnectorTransportEvent.Disconnected(0u))
                    gatt.close()
                    if (this@MavBleExecutor.gatt === gatt) this@MavBleExecutor.gatt = null
                }
            }
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            if (status == BluetoothGatt.GATT_SUCCESS) {
                eventSink(ConnectorTransportEvent.MtuChanged(mtu.coerceIn(0, UShort.MAX_VALUE.toInt()).toUShort()))
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            if (status == BluetoothGatt.GATT_SUCCESS) {
                eventSink(ConnectorTransportEvent.ServicesDiscovered(gatt.services.map { connectorWireUuid(it.uuid) }))
            } else {
                fail(null, status.toUShort(), "Bluetooth service discovery failed.")
            }
        }

        override fun onDescriptorWrite(gatt: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            val nativeKey = key(descriptor.characteristic)
            val target = notificationTargets.remove(nativeKey) ?: run {
                handler.post(::finishSubscription)
                return
            }
            if (status != BluetoothGatt.GATT_SUCCESS) {
                fail(null, status.toUShort(), "Bluetooth subscription failed.")
            } else if (target.second) {
                eventSink(ConnectorTransportEvent.Subscribed(target.first))
            } else {
                eventSink(ConnectorTransportEvent.Unsubscribed(target.first))
            }
            handler.post(::finishSubscription)
        }

        @Deprecated("Called through Android 12")
        override fun onCharacteristicRead(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            finishRead(characteristic, characteristic.value ?: byteArrayOf(), status)
        }

        override fun onCharacteristicRead(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
            status: Int,
        ) {
            finishRead(characteristic, value, status)
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            val pending = pendingWrites.remove(key(characteristic)) ?: return
            if (status == BluetoothGatt.GATT_SUCCESS) {
                eventSink(ConnectorTransportEvent.WriteResult(pending.first, pending.second))
            } else {
                fail(pending.first, status.toUShort(), "Bluetooth write failed.")
            }
        }

        @Deprecated("Called through Android 12")
        override fun onCharacteristicChanged(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic) {
            finishNotification(characteristic, characteristic.value ?: byteArrayOf())
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
        ) {
            finishNotification(characteristic, value)
        }
    }

    private fun finishRead(
        characteristic: BluetoothGattCharacteristic,
        value: ByteArray,
        status: Int,
    ) {
        val pending = pendingReads.remove(key(characteristic)) ?: return
        if (status == BluetoothGatt.GATT_SUCCESS) {
            eventSink(ConnectorTransportEvent.ReadResult(pending.first, pending.second, value))
        } else {
            fail(pending.first, status.toUShort(), "Bluetooth read failed.")
        }
    }

    private fun finishNotification(characteristic: BluetoothGattCharacteristic, value: ByteArray) {
        val logicalId = logicalIds[key(characteristic)] ?: characteristic.uuid.toString()
        eventSink(ConnectorTransportEvent.Notification(logicalId, value))
    }
}
