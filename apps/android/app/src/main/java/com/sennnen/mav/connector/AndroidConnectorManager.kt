package com.sennnen.mav.connector

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import com.sennnen.mav.BuildConfig
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.TimeZone
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.mav_ffi.ConnectorInspection
import uniffi.mav_ffi.ConnectorInstallRequest
import uniffi.mav_ffi.ConnectorRemovalMode
import uniffi.mav_ffi.ConnectorSessionConfig
import uniffi.mav_ffi.ConnectorTransportEvent
import uniffi.mav_ffi.ConnectorTrustPolicy
import uniffi.mav_ffi.ConnectorTrustRevocations
import uniffi.mav_ffi.FfiException
import uniffi.mav_ffi.InstalledConnectorRecord
import uniffi.mav_ffi.MavRuntime
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

    private val mutablePhase = MutableStateFlow<ConnectorApprovalPhase>(ConnectorApprovalPhase.Idle)
    val phase: StateFlow<ConnectorApprovalPhase> = mutablePhase.asStateFlow()

    private val mutableInstalled = MutableStateFlow<List<InstalledConnectorRecord>>(emptyList())
    val installed: StateFlow<List<InstalledConnectorRecord>> = mutableInstalled.asStateFlow()

    val managerEnabled: Boolean = BuildConfig.MAV_CONNECTOR_MANAGER_ENABLED
    val remoteImportEnabled: Boolean = BuildConfig.MAV_ALLOW_REMOTE_CONNECTORS
    val thirdPartyEnabled: Boolean = BuildConfig.MAV_ALLOW_THIRD_PARTY_CONNECTORS

    private val policy = ConnectorTrustPolicy(
        revision = 1uL,
        allowThirdParty = thirdPartyEnabled,
        allowDevelopment = BuildConfig.DEBUG,
        keys = emptyList(),
    )
    private val revocations = ConnectorTrustRevocations(
        revision = 0uL,
        generatedAtMs = 0,
        validUntilMs = System.currentTimeMillis() + 31_536_000_000,
        entries = emptyList(),
    )
    private val bluetooth = MavBleExecutor(appContext, ::applyTransportEvent)

    suspend fun start() {
        gate.withLock { ensureRuntime() }
        refreshInstalledNow()
        if (!sessionRestored) {
            sessionRestored = true
            bluetooth.checkpoint?.let { resumeSession(it) }
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
        scope.launch(Dispatchers.IO) {
            runCatching { resumeSession(checkpoint) }.onFailure(::fail)
        }
    }

    fun reportFailure(error: Throwable) = fail(error)

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

    private suspend fun resumeSession(checkpoint: ConnectorRestorationCheckpoint) {
        val actions = gate.withLock {
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
            active.drainConnectorActions(64u)
        }
        withContext(Dispatchers.Main) { actions.forEach(bluetooth::execute) }
    }

    private fun applyTransportEvent(event: ConnectorTransportEvent) {
        scope.launch(Dispatchers.IO) {
            runCatching {
                gate.withLock {
                    val active = ensureRuntime()
                    active.applyConnectorEvent(event, System.currentTimeMillis())
                    active.drainConnectorActions(64u)
                }
            }.onSuccess { actions ->
                withContext(Dispatchers.Main) { actions.forEach(bluetooth::execute) }
            }.onFailure(::fail)
        }
    }

    private fun ensureRuntime(): MavRuntime {
        runtime?.let { return it }
        val database = File(appContext.noBackupFilesDir, "mav.sqlite")
        return MavRuntime(
            RuntimeConfig(
                databasePath = database.absolutePath,
                timezoneId = TimeZone.getDefault().id,
                transportCapacity = 256u,
                appVersion = BuildConfig.VERSION_NAME,
                appBuild = BuildConfig.VERSION_CODE.toString(),
            ),
        ).also { runtime = it }
    }

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

    @Suppress("DEPRECATION")
    private fun Intent.readStreamUri(): Uri? =
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            getParcelableExtra(Intent.EXTRA_STREAM)
        }
}
