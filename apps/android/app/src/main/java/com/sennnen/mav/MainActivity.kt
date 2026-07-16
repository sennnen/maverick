package com.sennnen.mav

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.aura.AuraRootScreen

class MainActivity : ComponentActivity() {
    private val model: AppViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        AppearancePrefs.load(this)
        enableEdgeToEdge()
        setContent {
            AuraRootScreen(viewModel = model)
        }
    }

    override fun onResume() {
        super.onResume()
        model.refresh()
    }
}
