package com.sennnen.mav.connector

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import com.sennnen.mav.BuildConfig
import com.sennnen.mav.ecg.MavEcgClassifier
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.TimeZone
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.mav_ffi.ConnectorInspection
import uniffi.mav_ffi.ConnectorInstallRequest
import uniffi.mav_ffi.ConnectorCancelReason
import uniffi.mav_ffi.ConnectorLifecycleState
import uniffi.mav_ffi.ConnectorRemovalMode
import uniffi.mav_ffi.ConnectorRegistryCheckpoint
import uniffi.mav_ffi.ConnectorRegistryEntry
import uniffi.mav_ffi.ConnectorSessionConfig
import uniffi.mav_ffi.ConnectorTransportEvent
import uniffi.mav_ffi.ConnectorTrustPolicy
import uniffi.mav_ffi.ConnectorTrustRevocations
import uniffi.mav_ffi.FfiException
import uniffi.mav_ffi.InstalledConnectorRecord
import uniffi.mav_ffi.DailySnapshotReport
import uniffi.mav_ffi.ConnectorCaptureCapability
import uniffi.mav_ffi.EcgCaptureReport
import uniffi.mav_ffi.EcgPrediction
import uniffi.mav_ffi.EcgReportPayload
import uniffi.mav_ffi.EcgResultReport
import uniffi.mav_ffi.MavRuntime
import uniffi.mav_ffi.TimezoneSpan
import uniffi.mav_ffi.RuntimeConfig

class AndroidConnectorManager(
    context: Context,
    private val scope: CoroutineScope,
) : AutoCloseable {
    private val appContext = context.applicationContext
    private val gate = Mutex()
    private var runtime: MavRuntime? = null
    private var sessionRestored = false
    private var inspection: ConnectorInspection? = null
    private var acquisition: ConnectorAcquisition? = null
    private val machine = ConnectorApprovalMachine()
    private val registryConfiguration = AndroidRegistryConfiguration.current()
    private val registryCache = ConnectorRegistryCache(appContext)
    private var registryCheckpoint: ConnectorRegistryCheckpoint? = null

    private val mutablePhase = MutableStateFlow<ConnectorApprovalPhase>(ConnectorApprovalPhase.Idle)
    val phase: StateFlow<ConnectorApprovalPhase> = mutablePhase.asStateFlow()

    private val mutableInstalled = MutableStateFlow<List<InstalledConnectorRecord>>(emptyList())
    val installed: StateFlow<List<InstalledConnectorRecord>> = mutableInstalled.asStateFlow()

    private val mutableRegistryEntries = MutableStateFlow<List<ConnectorRegistryEntry>>(emptyList())
    val registryEntries: StateFlow<List<ConnectorRegistryEntry>> = mutableRegistryEntries.asStateFlow()

    private val mutableRegistryError = MutableStateFlow<String?>(null)
    val registryError: StateFlow<String?> = mutableRegistryError.asStateFlow()

    private val mutableConnection = MutableStateFlow(ConnectorConnectionState())
    val connection: StateFlow<ConnectorConnectionState> = mutableConnection.asStateFlow()

    private val mutableDiscoveredDevices = MutableStateFlow<List<ConnectorScanDevice>>(emptyList())
    val discoveredDevices: StateFlow<List<ConnectorScanDevice>> = mutableDiscoveredDevices.asStateFlow()

    private val mutableEcgCapabilities =
        MutableStateFlow<List<ConnectorCaptureCapability>>(emptyList())
    val ecgCapabilities: StateFlow<List<ConnectorCaptureCapability>> =
        mutableEcgCapabilities.asStateFlow()

    private val mutableEcgCapture = MutableStateFlow<EcgCaptureReport?>(null)
    val ecgCapture: StateFlow<EcgCaptureReport?> = mutableEcgCapture.asStateFlow()

    private val mutableEcgResults = MutableStateFlow<List<EcgResultReport>>(emptyList())
    val ecgResults: StateFlow<List<EcgResultReport>> = mutableEcgResults.asStateFlow()

    private val mutableEcgError = MutableStateFlow<String?>(null)
    val ecgError: StateFlow<String?> = mutableEcgError.asStateFlow()
    private var ecgInferenceInFlight: ULong? = null

    val managerEnabled: Boolean = BuildConfig.MAV_CONNECTOR_MANAGER_ENABLED
    val remoteImportEnabled: Boolean = BuildConfig.MAV_ALLOW_REMOTE_CONNECTORS
    val thirdPartyEnabled: Boolean = BuildConfig.MAV_ALLOW_THIRD_PARTY_CONNECTORS

    private var policy = ConnectorTrustPolicy(
        revision = 1uL,
        allowThirdParty = thirdPartyEnabled,
        allowDevelopment = BuildConfig.MAV_ALLOW_DEVELOPMENT_CONNECTORS,
        keys = AndroidConnectorTrust.configuredKeys(),
    )
    private var revocations = ConnectorTrustRevocations(
        revision = 0uL,
        generatedAtMs =
            if (registryConfiguration == null || BuildConfig.MAV_ALLOW_DEVELOPMENT_CONNECTORS) 0 else 1,
        validUntilMs = if (
            registryConfiguration == null || BuildConfig.MAV_ALLOW_DEVELOPMENT_CONNECTORS
        ) {
            System.currentTimeMillis() + 31_536_000_000
        } else {
            0
        },
        entries = emptyList(),
    )
    private val transportEvents = Channel<ConnectorTransportEvent>(Channel.UNLIMITED)
    private val bluetooth = MavBleExecutor(
        appContext,
        ::enqueueTransportEvent,
        { mutableDiscoveredDevices.value = it },
        { failConnection(IllegalStateException(it)) },
    )

    init {
        scope.launch(Dispatchers.IO) {
            for (event in transportEvents) applyTransportEvent(event)
        }
    }

    suspend fun start() {
        gate.withLock { ensureRuntime() }
        restoreRegistryCache()
        refreshRegistry()
        refreshInstalledNow()
        refreshEcgHistory()
        if (!sessionRestored) {
            sessionRestored = true
            bluetooth.checkpoint?.let { resumeSession(it) }
        }
    }

    fun refreshRegistry() {
        val configuration = registryConfiguration ?: return
        scope.launch(Dispatchers.IO) {
            runCatching {
                val connection = URL(configuration.url).openConnection() as HttpURLConnection
                connection.connectTimeout = 15_000
                connection.readTimeout = 30_000
                connection.instanceFollowRedirects = false
                connection.requestMethod = "GET"
                try {
                    if (connection.responseCode !in 200..299) throw ConnectorAcquisitionException.InvalidResponse()
                    if (connection.contentLengthLong > BoundedRegistryReader.MAXIMUM_BYTES) {
                        throw ConnectorAcquisitionException.TooLarge()
                    }
                    connection.inputStream.use(BoundedRegistryReader::read)
                } finally {
                    connection.disconnect()
                }
            }.mapCatching { bytes ->
                gate.withLock {
                    val snapshot = ensureRuntime().ingestConnectorRegistry(
                        bytes,
                        configuration.root,
                        registryCheckpoint,
                        policy,
                        System.currentTimeMillis(),
                    )
                    applyRegistrySnapshot(bytes, snapshot)
                }
            }.onSuccess {
                mutableRegistryError.value = null
            }.onFailure(::failRegistry)
        }
    }

    fun importRegistryEntry(entry: ConnectorRegistryEntry) {
        if (!remoteImportEnabled || entry.revoked || !entry.artifactUrl.startsWith("https://")) {
            fail("This registry connector is not available for remote import.")
            return
        }
        scope.launch(Dispatchers.IO) {
            runCatching {
                val connection = URL(entry.artifactUrl).openConnection() as HttpURLConnection
                connection.connectTimeout = 15_000
                connection.readTimeout = 30_000
                connection.instanceFollowRedirects = true
                try {
                    connection.connect()
                    if (!connection.url.protocol.equals("https", ignoreCase = true) ||
                        connection.responseCode !in 200..299
                    ) {
                        throw ConnectorAcquisitionException.InvalidResponse()
                    }
                    if (connection.contentLengthLong > ConnectorAcquisition.MAXIMUM_BYTES.toLong()) {
                        throw ConnectorAcquisitionException.TooLarge()
                    }
                    connection.inputStream.use(BoundedConnectorReader::read)
                } finally {
                    connection.disconnect()
                }
            }.mapCatching { bytes ->
                gate.withLock { ensureRuntime().verifyConnectorRegistryArtifact(entry, bytes) }
                ConnectorAcquisition.make(
                    bytes,
                    ConnectorImportOrigin.REMOTE,
                    "${entry.connectorId}-${entry.version}.mavconn",
                    entry.artifactUrl,
                )
            }.onSuccess { inspect(it) }.onFailure(::fail)
        }
    }

    fun handleIntent(intent: Intent?) {
        if (intent == null) return
        when (intent.action) {
            Intent.ACTION_VIEW -> intent.data?.let { uri ->
                if (uri.scheme.equals("https", ignoreCase = true)) importRemote(uri.toString())
                else importUri(uri, ConnectorImportOrigin.CONTENT)
            }
            Intent.ACTION_SEND -> {
                val uri = intent.readStreamUri()
                if (uri != null) importUri(uri, ConnectorImportOrigin.SHARE)
                else intent.getStringExtra(Intent.EXTRA_TEXT)?.let(::importRemote)
            }
        }
    }

    fun importUri(uri: Uri, origin: ConnectorImportOrigin = ConnectorImportOrigin.CONTENT) {
        if (!managerEnabled) {
            fail("Connector management is disabled in this release.")
            return
        }
        if (uri.scheme.equals("https", ignoreCase = true)) {
            importRemote(uri.toString())
            return
        }
        if (uri.scheme != "content" && uri.scheme != "file") {
            fail(ConnectorAcquisitionException.UnsupportedUri().message.orEmpty())
            return
        }
        scope.launch(Dispatchers.IO) {
            runCatching {
                val length = appContext.contentResolver.openAssetFileDescriptor(uri, "r")?.use { it.length }
                if (length != null && length > ConnectorAcquisition.MAXIMUM_BYTES.toLong()) {
                    throw ConnectorAcquisitionException.TooLarge()
                }
                val bytes = appContext.contentResolver.openInputStream(uri)?.use(BoundedConnectorReader::read)
                    ?: throw ConnectorAcquisitionException.Empty()
                ConnectorAcquisition.make(
                    bytes = bytes,
                    origin = origin,
                    displayName = displayName(uri),
                    locator = uri.toString(),
                )
            }.onSuccess { inspect(it) }.onFailure(::fail)
        }
    }

    fun importRemote(value: String) {
        if (!managerEnabled) {
            fail("Connector management is disabled in this release.")
            return
        }
        if (!remoteImportEnabled) {
            fail(ConnectorAcquisitionException.RemoteDisabled().message.orEmpty())
            return
        }
        val url = runCatching { URL(value) }.getOrNull()
        if (url == null || !url.protocol.equals("https", ignoreCase = true)) {
            fail(ConnectorAcquisitionException.UnsupportedUri().message.orEmpty())
            return
        }
        scope.launch(Dispatchers.IO) {
            runCatching {
                val connection = url.openConnection() as HttpURLConnection
                connection.connectTimeout = 15_000
                connection.readTimeout = 30_000
                connection.instanceFollowRedirects = true
                connection.useCaches = false
                try {
                    connection.connect()
                    if (!connection.url.protocol.equals("https", ignoreCase = true) ||
                        connection.responseCode !in 200..299
                    ) {
                        throw ConnectorAcquisitionException.InvalidResponse()
                    }
                    if (connection.contentLengthLong > ConnectorAcquisition.MAXIMUM_BYTES.toLong()) {
                        throw ConnectorAcquisitionException.TooLarge()
                    }
                    ConnectorAcquisition.make(
                        bytes = connection.inputStream.use(BoundedConnectorReader::read),
                        origin = ConnectorImportOrigin.REMOTE,
                        displayName = connection.getHeaderField("Content-Disposition")
                            ?.substringAfter("filename=", "")
                            ?.trim(' ', '"')
                            ?.takeIf(String::isNotBlank)
                            ?: connection.url.path.substringAfterLast('/').ifBlank { "Connector" },
                        locator = connection.url.toString(),
                    )
                } finally {
                    connection.disconnect()
                }
            }.onSuccess { inspect(it) }.onFailure(::fail)
        }
    }

    fun approve() {
        runCatching { machine.beginApproval() }.onFailure {
            fail(it)
            return
        }
        mutablePhase.value = machine.phase
        val pendingInspection = inspection
        val pendingAcquisition = acquisition
        if (pendingInspection == null || pendingAcquisition == null) {
            fail("Inspection expired. Import the connector again.")
            return
        }
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    ensureRuntime().installConnectorBytes(
                        request = ConnectorInstallRequest(
                            bytes = pendingAcquisition.bytes,
                            source = pendingAcquisition.source,
                            approvalToken = pendingInspection.approvalToken,
                            activate = true,
                            nowMs = System.currentTimeMillis(),
                        ),
                        policy = policy,
                        revocations = revocations,
                    )
                }
            }.onSuccess { record ->
                inspection = null
                acquisition = null
                machine.installed(record.connectorId)
                mutablePhase.value = machine.phase
                refreshInstalledNow()
            }.onFailure(::fail)
        }
    }

    fun cancel() {
        inspection = null
        acquisition = null
        machine.cancel()
        mutablePhase.value = machine.phase
    }

    fun rollback(connectorId: String) {
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    ensureRuntime().rollbackInstalledConnector(
                        connectorId, policy, revocations, System.currentTimeMillis(),
                    )
                }
            }.onSuccess {
                machine.rolledBack(connectorId)
                mutablePhase.value = machine.phase
                refreshInstalledNow()
            }.onFailure(::fail)
        }
    }

    fun remove(record: InstalledConnectorRecord) {
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    ensureRuntime().removeInstalledConnector(
                        record.connectorId,
                        record.version,
                        ConnectorRemovalMode.QUARANTINE_STATE,
                        policy,
                        revocations,
                        System.currentTimeMillis(),
                    )
                }
            }.onSuccess {
                cancel()
                refreshInstalledNow()
            }.onFailure(::fail)
        }
    }

    fun enforceRevocations() {
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    ensureRuntime().enforceConnectorTrust(policy, revocations, System.currentTimeMillis())
                }
            }.onSuccess { disabled ->
                disabled.firstOrNull()?.let(machine::revoked)
                mutablePhase.value = machine.phase
                refreshInstalledNow()
            }.onFailure(::fail)
        }
    }

    fun connect(record: InstalledConnectorRecord) {
        val checkpoint = ConnectorRestorationCheckpoint(
            connectorId = record.connectorId,
            sessionId = System.currentTimeMillis().coerceAtLeast(1).toULong(),
            cancellationGeneration = 0uL,
        )
        bluetooth.checkpoint = checkpoint
        mutableConnection.value = ConnectorConnectionState(
            connectorId = record.connectorId,
            label = "Starting",
        )
        scope.launch(Dispatchers.IO) {
            runCatching { resumeSession(checkpoint) }.onFailure(::failConnection)
        }
    }

    fun disconnect() {
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    val active = ensureRuntime()
                    active.cancelConnectorSession(
                        ConnectorCancelReason.USER,
                        System.currentTimeMillis(),
                    )
                    active.drainConnectorActions(64u) to active.connectorTelemetry(System.currentTimeMillis())
                }
            }.onSuccess { (actions, telemetry) ->
                publishTelemetry(telemetry)
                withContext(Dispatchers.Main) { actions.forEach(bluetooth::execute) }
                if (telemetry.lifecycle == ConnectorLifecycleState.DISCONNECTED) {
                    bluetooth.checkpoint = null
                }
            }.onFailure(::failConnection)
        }
    }

    fun selectDevice(deviceId: String) = bluetooth.selectDevice(deviceId)

    fun startEcgCapture() {
        if (mutableEcgCapabilities.value.none { it.stream == "ecg" }) {
            mutableEcgError.value =
                "This connected device has not positively declared ECG capture."
            return
        }
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    ensureRuntime().startConnectorCapture("ecg", System.currentTimeMillis())
                }
                refreshEcgState()
            }.onFailure { mutableEcgError.value = it.userMessage() }
        }
    }

    fun stopEcgCapture() {
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    ensureRuntime().stopConnectorCapture("ecg", System.currentTimeMillis())
                }
                refreshEcgState()
            }.onFailure { mutableEcgError.value = it.userMessage() }
        }
    }

    /** Forget one reading. The history reloads from the store rather than being patched. */
    fun removeEcgResult(captureId: ULong) {
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock { ensureRuntime().deleteEcgCapture(captureId) }
            }.onSuccess { refreshEcgHistory() }
                .onFailure { mutableEcgError.value = it.userMessage() }
        }
    }

    suspend fun ecgReportPayload(captureId: ULong): EcgReportPayload? =
        withContext(Dispatchers.IO) {
            gate.withLock { ensureRuntime().ecgReportPayload(captureId) }
        }

    fun reportFailure(error: Throwable) = failConnection(error)

    fun onBluetoothPermissionResult(granted: Boolean) = bluetooth.onPermissionResult(granted)

    override fun close() {
        bluetooth.close()
        runtime?.close()
        runtime = null
        sessionRestored = false
    }

    private suspend fun inspect(payload: ConnectorAcquisition) {
        machine.beginInspection()
        mutablePhase.value = machine.phase
        runCatching {
            gate.withLock {
                ensureRuntime().inspectConnectorBytes(
                    bytes = payload.bytes,
                    source = payload.source,
                    policy = policy,
                    revocations = revocations,
                    nowMs = System.currentTimeMillis(),
                    approvalTtlMs = 300_000,
                )
            }
        }.onSuccess { report ->
            inspection = report
            acquisition = payload
            machine.inspectionSucceeded(
                ConnectorApprovalSummary(
                    connectorId = report.connectorId,
                    version = report.version,
                    displayName = report.displayName,
                    publisherKeyId = report.publisherKeyId,
                    fixtureCount = report.fixtureCount,
                    detail = report.description,
                    sourceName = report.source.displayName,
                    capabilities = report.capabilities,
                    permissions = report.permissions,
                ),
                payload.bytes,
            )
            mutablePhase.value = machine.phase
        }.onFailure(::fail)
    }

    private suspend fun refreshInstalledNow() {
        runCatching {
            gate.withLock { ensureRuntime().listInstalledConnectors() }
        }.onSuccess { mutableInstalled.value = it }.onFailure(::fail)
    }

    private suspend fun restoreRegistryCache() {
        val configuration = registryConfiguration ?: return
        val cached = registryCache.load() ?: return
        runCatching {
            gate.withLock {
                ensureRuntime().restoreConnectorRegistry(
                    cached.bytes,
                    configuration.root,
                    cached.checkpoint,
                    policy,
                    System.currentTimeMillis(),
                )
            }
        }.onSuccess { snapshot ->
            applyRegistrySnapshot(cached.bytes, snapshot)
            mutableRegistryError.value = null
        }.onFailure(::failRegistry)
    }

    private fun applyRegistrySnapshot(bytes: ByteArray, snapshot: uniffi.mav_ffi.ConnectorRegistrySnapshot) {
        policy = snapshot.trust
        revocations = snapshot.revocations
        registryCheckpoint = snapshot.checkpoint
        mutableRegistryEntries.value = snapshot.entries
        registryCache.save(bytes, snapshot.checkpoint)
    }

    private suspend fun resumeSession(checkpoint: ConnectorRestorationCheckpoint) {
        val (actions, telemetry) = gate.withLock {
            val active = ensureRuntime()
            active.openConnectorSession(
                ConnectorSessionConfig(
                    connectorId = checkpoint.connectorId,
                    sessionId = checkpoint.sessionId,
                    deviceId = 1uL,
                    transportCapacity = 256u,
                    nowMs = System.currentTimeMillis(),
                ),
                policy,
                revocations,
            )
            active.drainConnectorActions(64u) to active.connectorTelemetry(System.currentTimeMillis())
        }
        publishTelemetry(telemetry)
        withContext(Dispatchers.Main) { actions.forEach(bluetooth::execute) }
        refreshEcgState()
    }

    private fun enqueueTransportEvent(event: ConnectorTransportEvent) {
        if (transportEvents.trySend(event).isFailure) {
            failConnection(IllegalStateException("Bluetooth event queue is unavailable."))
        }
    }

    private suspend fun applyTransportEvent(event: ConnectorTransportEvent) {
        runCatching {
            gate.withLock {
                val active = ensureRuntime()
                active.applyConnectorEvent(event, System.currentTimeMillis())
                active.drainConnectorActions(64u) to active.connectorTelemetry(System.currentTimeMillis())
            }
        }.onSuccess { (actions, telemetry) ->
            publishTelemetry(telemetry)
            withContext(Dispatchers.Main) { actions.forEach(bluetooth::execute) }
            refreshEcgState()
            if (telemetry.lifecycle == ConnectorLifecycleState.DISCONNECTED && actions.isEmpty()) {
                bluetooth.checkpoint = null
            }
        }.onFailure(::failConnection)
    }

    private fun publishTelemetry(telemetry: uniffi.mav_ffi.ConnectorTelemetrySnapshot) {
        bluetooth.checkpoint = ConnectorRestorationCheckpoint(
            connectorId = telemetry.connectorId,
            sessionId = telemetry.sessionId,
            cancellationGeneration = telemetry.cancellationGeneration,
        )
        val counts = runCatching { ensureRuntime().connectorLifecycle() }.getOrNull()
        mutableConnection.value = ConnectorConnectionState.from(telemetry).copy(
            samplesPersisted = counts?.samplesPersisted?.toLong() ?: 0L,
            samplesDuplicate = counts?.samplesDuplicate?.toLong() ?: 0L,
        )
    }

    private suspend fun refreshEcgHistory() {
        runCatching {
            gate.withLock { ensureRuntime().ecgResults(1uL, 50u) }
        }.onSuccess { mutableEcgResults.value = it }
    }

    private suspend fun refreshEcgState() {
        val state = runCatching {
            gate.withLock {
                val active = ensureRuntime()
                Triple(
                    active.connectorCaptureCapabilities(),
                    active.ecgCaptureState(System.currentTimeMillis()),
                    active.ecgInferenceRequest(),
                )
            }
        }.getOrElse {
            mutableEcgCapabilities.value = emptyList()
            return
        }
        mutableEcgCapabilities.value = state.first
        mutableEcgCapture.value = state.second
        val request = state.third
        if (state.second?.phase != "analysing" ||
            request == null ||
            ecgInferenceInFlight == request.captureId
        ) {
            return
        }
        ecgInferenceInFlight = request.captureId
        runCatching {
            val predictions = MavEcgClassifier(appContext).use { classifier ->
                classifier.predictBatch(request.tensors.map { it.values.toFloatArray() }).map {
                    EcgPrediction(
                        sinusRhythm = it[0],
                        atrialFibrillation = it[1],
                        otherAbnormalRhythm = it[2],
                    )
                }
            }
            gate.withLock {
                ensureRuntime().submitEcgInference(
                    request.captureId,
                    predictions,
                    MavEcgClassifier.ADMITTED_MODEL_SHA256,
                    System.currentTimeMillis(),
                )
            }
        }.onSuccess { result ->
            mutableEcgCapture.value = EcgCaptureReport(
                captureId = result.captureId,
                phase = "result",
                progressMilli = 1_000u.toUShort(),
                qualityMilli = result.qualityMilli,
                qualityReason = null,
                recordedSamples = result.sampleCount,
                targetSamples = result.sampleCount,
            )
            mutableEcgError.value = null
            refreshEcgHistory()
        }.onFailure { mutableEcgError.value = it.userMessage() }
        ecgInferenceInFlight = null
    }

    private fun ensureRuntime(): MavRuntime {
        runtime?.let { return it }
        val database = File(appContext.noBackupFilesDir, "mav.sqlite")
        return MavRuntime(
            RuntimeConfig(
                databasePath = database.absolutePath,
                timezoneId = TimeZone.getDefault().id,
                appVersion = "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
            ),
        ).also {
            runtime = it
            it.setTimezoneSpans(TimeZone.getDefault().id, offsetSpans(TimeZone.getDefault()))
            com.sennnen.mav.ui.AuraZoneMath.runtime = it
            com.sennnen.mav.ui.MavRepo.sharedRuntime = it
            installBundledConnector(it)
        }
    }

    /**
     * Install the shipped Generic HR Monitor the first time the runtime opens, so a fresh install
     * can pair with a chest strap before the wearer has found any connector at all.
     *
     * It goes through the same public path every other connector uses — inspect, then install
     * against the approval token that inspection issued — because a bundled artifact that skipped
     * verification would be a second trust path, and the whole point is that there is only one.
     * Already installed is the normal case and is silent.
     */
    private fun installBundledConnector(open: MavRuntime) {
        if (open.listInstalledConnectors().any { it.connectorId == BundledConnector.CONNECTOR_ID }) {
            return
        }
        runCatching {
            val bytes = appContext.assets.open(BundledConnector.ASSET).use { it.readBytes() }
            val acquisition = ConnectorAcquisition.make(
                bytes = bytes,
                origin = ConnectorImportOrigin.BUNDLED,
                displayName = BundledConnector.DISPLAY_NAME,
                locator = BundledConnector.ASSET,
            )
            val now = System.currentTimeMillis()
            val inspected = open.inspectConnectorBytes(
                bytes = acquisition.bytes,
                source = acquisition.source,
                policy = policy,
                revocations = revocations,
                nowMs = now,
                approvalTtlMs = 60_000,
            )
            open.installConnectorBytes(
                request = ConnectorInstallRequest(
                    bytes = acquisition.bytes,
                    source = acquisition.source,
                    approvalToken = inspected.approvalToken,
                    activate = true,
                    nowMs = now,
                ),
                policy = policy,
                revocations = revocations,
            )
        }
    }

    /**
     * The platform's own zone database, flattened into the explicit spans the core buckets days by.
     * Rust holds no tzdata (ADR-024): the phone has a correct and updated one, and it is the only
     * place the user's zone is genuinely known. Two years back and one forward covers every day the
     * app can show plus the next transition.
     */
    private fun offsetSpans(zone: TimeZone): List<TimezoneSpan> {
        val day = 86_400L
        val now = System.currentTimeMillis() / 1000L
        var cursor = now - 730 * day
        val end = now + 365 * day
        val spans = mutableListOf<TimezoneSpan>()
        var last: Int? = null
        while (cursor <= end) {
            val offset = zone.getOffset(cursor * 1000L) / 1000
            if (offset != last) {
                spans.add(TimezoneSpan(startUnixSeconds = cursor, offsetSeconds = offset))
                last = offset
            }
            cursor += day
        }
        if (spans.isEmpty()) {
            spans.add(TimezoneSpan(startUnixSeconds = 0L, offsetSeconds = zone.rawOffset / 1000))
        }
        return spans
    }

    /**
     * The diagnostics bundle as JSON: app build, live session, trace hash, commit totals, and the
     * recent pipeline boundaries. Counts and hashes only — the ring log holds no sample values and
     * payload summaries exist only in debug builds — so this is safe to share as it stands.
     */
    fun reportBundleJson(limit: UInt = 200u): String? = runCatching {
        val bundle = ensureRuntime().exportReportBundle(limit)
        buildString {
            append("{\n")
            append("  \"app_version\": \"${bundle.appVersion}\",\n")
            append("  \"connector_id\": ${bundle.connectorId?.let { "\"$it\"" } ?: "null"},\n")
            append("  \"session_id\": ${bundle.sessionId ?: "null"},\n")
            append("  \"trace_hash\": ${bundle.traceHash?.let { "\"$it\"" } ?: "null"},\n")
            append("  \"samples_persisted\": ${bundle.samplesPersisted},\n")
            append("  \"samples_duplicate\": ${bundle.samplesDuplicate},\n")
            append("  \"recent_stages\": [\n")
            bundle.recentStages.forEachIndexed { index, stage ->
                append("    {\"seq\": ${stage.seq}, \"stage\": \"${stage.stage}\", ")
                append("\"kind\": \"${stage.kind}\", \"count\": ${stage.count}, ")
                append("\"detail\": \"${stage.detail.replace("\"", "'")}\"}")
                if (index < bundle.recentStages.lastIndex) append(",")
                append("\n")
            }
            append("  ]\n}\n")
        }
    }.getOrNull()

    /** One local day's analytics from the core, or null when no runtime is open yet. */
    fun dailySnapshot(deviceId: ULong, wallTimeMs: Long): DailySnapshotReport? =
        runCatching { ensureRuntime().dailySnapshot(deviceId, wallTimeMs) }.getOrNull()

    private fun displayName(uri: Uri): String {
        if (uri.scheme == "file") return uri.lastPathSegment ?: "Connector"
        return appContext.contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            if (cursor.moveToFirst()) cursor.getString(0) else null
        } ?: uri.lastPathSegment ?: "Connector"
    }

    private fun fail(error: Throwable) = fail(
        when (error) {
            is FfiException.Core -> error.safeMessage
            else -> error.message ?: "Connector operation failed."
        },
    )

    private fun fail(message: String) {
        machine.fail(message)
        mutablePhase.value = machine.phase
    }

    private fun failRegistry(error: Throwable) {
        mutableRegistryError.value = error.userMessage()
    }

    private fun failConnection(error: Throwable) {
        val message = error.userMessage()
        mutableConnection.value = mutableConnection.value.copy(
            lifecycle = ConnectorLifecycleState.FAILED,
            label = "Failed",
            connected = false,
            heartRateBpm = null,
            batteryPercent = null,
            onWrist = null,
            lastSampleWallTimeMs = null,
            errorMessage = message,
        )
    }

    private fun Throwable.userMessage(): String = when (this) {
        is FfiException.Core -> safeMessage
        else -> message ?: "Connector operation failed."
    }

    @Suppress("DEPRECATION")
    private fun Intent.readStreamUri(): Uri? =
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            getParcelableExtra(Intent.EXTRA_STREAM)
        }
}
