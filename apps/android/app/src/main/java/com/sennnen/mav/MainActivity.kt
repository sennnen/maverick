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
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.mav.MavRootScreen

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
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        model.connectors.handleIntent(intent)
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
