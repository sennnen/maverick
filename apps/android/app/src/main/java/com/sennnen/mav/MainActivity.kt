package com.sennnen.mav

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val model: MavViewModel = viewModel()
            val state by model.state.collectAsStateWithLifecycle()
            MaterialTheme(
                colorScheme = darkColorScheme(
                    background = Color.Black,
                    surface = Color(0xFF141416),
                    primary = Color(0xFFE7FE55),
                ),
            ) {
                DiagnosticScreen(state = state, onRefresh = model::refresh)
            }
        }
    }
}

@Composable
private fun DiagnosticScreen(state: MavAppState, onRefresh: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .padding(horizontal = 24.dp, vertical = 64.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Mav", style = MaterialTheme.typography.headlineLarge)
        when (state) {
            MavAppState.Loading -> Text("Opening local core…")
            is MavAppState.Failed -> {
                Text(state.code, color = MaterialTheme.colorScheme.error)
                Text(state.message)
            }
            is MavAppState.Ready -> {
                Text("Core ${state.snapshot.coreVersion}")
                Text("Storage schema ${state.snapshot.storageSchema}")
                Text("Connection ${state.snapshot.connectionState}")
                Text("Heart rate ${state.snapshot.currentBpm ?: "--"} bpm")
                Text("Snapshot ${state.snapshot.hash}")
            }
        }
        Button(onClick = onRefresh) {
            Text("Refresh")
        }
    }
}
