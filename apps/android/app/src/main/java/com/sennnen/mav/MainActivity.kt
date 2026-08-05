package com.sennnen.mav

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import com.sennnen.mav.ml.MavAnalyticsWorker
import com.sennnen.mav.ml.MavUsagePattern
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.mav.MavRootScreen
import java.util.Calendar

class MainActivity : ComponentActivity() {
    private val model: AppViewModel by viewModels()
    private val connectorDocument = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) model.connectors.importUri(uri)
    }
    private val bluetoothPermissions = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { result -> model.connectors.onBluetoothPermissionResult(result.values.all { it }) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        AppearancePrefs.load(this)
        requestBluetoothPermissions()
        MavAnalyticsWorker.schedulePeriodic(this)
        model.connectors.handleIntent(intent)
        enableEdgeToEdge()
        setContent {
            MavRootScreen(
                viewModel = model,
                onChooseConnectorFile = {
                    connectorDocument.launch(arrayOf("application/vnd.maverick.connector", "application/octet-stream"))
                },
            )
        }
    }

    override fun onResume() {
        super.onResume()
        model.refresh()
        rememberThisOpen()
    }

    override fun onPause() {
        super.onPause()
        // Aim one speculative pass at the wearer's next likely open. Best-effort: the OS decides
        // whether this ever runs, and the foreground pass in onResume is what actually
        // guarantees a result.
        val pattern = MavUsagePattern.parse(usagePreferences().getString(KEY_USAGE, null))
        val hourNow = Calendar.getInstance().get(Calendar.HOUR_OF_DAY)
        MavAnalyticsWorker.precomputeDelayMinutes(pattern, hourNow)?.let { delay ->
            MavAnalyticsWorker.schedulePrecompute(this, delay)
        }
    }

    override fun onStop() {
        super.onStop()
        // `onStop`, not `onPause`: a partially covered activity is still on screen, and dropping
        // the models behind a dialog would reload them the moment it closed. Queued behind any
        // pass in flight, so this never releases a model mid-inference.
        model.releaseAnalyticsResources()
    }

    /**
     * Note that the app was opened in this hour.
     *
     * Local only, never uploaded, and not a sensor reading — it is the app watching itself so
     * that a background pass can land before an open rather than after one.
     */
    private fun rememberThisOpen() {
        val preferences = usagePreferences()
        val hour = Calendar.getInstance().get(Calendar.HOUR_OF_DAY)
        val updated = MavUsagePattern.parse(preferences.getString(KEY_USAGE, null)).record(hour)
        preferences.edit().putString(KEY_USAGE, updated.encode()).apply()
    }

    private fun usagePreferences() = getSharedPreferences(USAGE_PREFERENCES, MODE_PRIVATE)

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        model.connectors.handleIntent(intent)
    }

    private companion object {
        const val USAGE_PREFERENCES = "mav-usage"
        const val KEY_USAGE = "open-hours"
    }

    private fun requestBluetoothPermissions() {
        val permissions = if (Build.VERSION.SDK_INT >= 31) {
            arrayOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)
        } else {
            arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }
        val missing = permissions.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isNotEmpty()) bluetoothPermissions.launch(missing.toTypedArray())
    }
}
