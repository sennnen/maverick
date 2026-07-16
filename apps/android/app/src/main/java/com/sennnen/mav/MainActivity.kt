package com.sennnen.mav

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.viewmodel.compose.viewModel
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.aura.AuraRootScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        AppearancePrefs.load(this)
        enableEdgeToEdge()
        setContent {
            val model: AppViewModel = viewModel()
            AuraRootScreen(viewModel = model)
        }
    }
}
