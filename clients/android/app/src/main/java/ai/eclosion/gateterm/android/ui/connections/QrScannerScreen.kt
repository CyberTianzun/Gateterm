package ai.eclosion.gateterm.android.ui.connections

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import ai.eclosion.gateterm.android.R
import ai.eclosion.gateterm.android.connection.ConnectionQr
import ai.eclosion.gateterm.android.connection.ScannedConnection
import ai.eclosion.gateterm.android.ui.components.AppBarTitle
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeCallback
import com.journeyapps.barcodescanner.BarcodeResult
import com.journeyapps.barcodescanner.CameraPreview
import com.journeyapps.barcodescanner.DecoratedBarcodeView
import com.journeyapps.barcodescanner.DefaultDecoderFactory

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QrScannerScreen(onScanned: (ScannedConnection) -> Unit, onBack: () -> Unit, onManual: () -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    fun allowed() = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
    var granted by remember { mutableStateOf(allowed()) }
    var invalid by remember { mutableStateOf(false) }
    var cameraFailed by remember { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted = it }
    LaunchedEffect(Unit) { if (!granted) permission.launch(Manifest.permission.CAMERA) }
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) granted = allowed() }
        owner.lifecycle.addObserver(observer)
        onDispose { owner.lifecycle.removeObserver(observer) }
    }
    Scaffold(topBar = {
        TopAppBar(title = { AppBarTitle(stringResource(R.string.qr_scan_title)) }, navigationIcon = {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back))
            }
        })
    }) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            if (granted) {
                Column(Modifier.fillMaxSize()) {
                    QrCamera(modifier = Modifier.weight(1f).fillMaxWidth(), onText = { text ->
                        val result = runCatching { ConnectionQr.parse(text) }.getOrNull()
                        if (result == null) { invalid = true; false }
                        else { onScanned(result); true }
                    }, onFailure = { cameraFailed = true })
                    Surface(modifier = Modifier.fillMaxWidth(), tonalElevation = 3.dp) {
                        Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(stringResource(R.string.qr_scan_hint), style = MaterialTheme.typography.bodyMedium)
                            if (invalid || cameraFailed) Text(
                                stringResource(if (cameraFailed) R.string.qr_camera_failed else R.string.qr_scan_invalid),
                                color = MaterialTheme.colorScheme.error,
                            )
                            TextButton(onClick = onManual) { Text(stringResource(R.string.qr_scan_manual)) }
                        }
                    }
                }
            } else {
                Column(Modifier.align(Alignment.Center).padding(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    Text(stringResource(R.string.qr_camera_permission))
                    Button(onClick = { permission.launch(Manifest.permission.CAMERA) }) { Text(stringResource(R.string.qr_allow_camera)) }
                    TextButton(onClick = {
                        context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}")))
                    }) { Text(stringResource(R.string.qr_open_settings)) }
                    TextButton(onClick = onManual) { Text(stringResource(R.string.qr_scan_manual)) }
                }
            }
        }
    }
}

@Composable
private fun QrCamera(modifier: Modifier, onText: (String) -> Boolean, onFailure: () -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val currentText by rememberUpdatedState(onText)
    val currentFailure by rememberUpdatedState(onFailure)
    val view = remember(context) { DecoratedBarcodeView(context).apply {
        setStatusText("") // The surrounding Compose surface supplies localized guidance.
        barcodeView.decoderFactory = DefaultDecoderFactory(listOf(BarcodeFormat.QR_CODE))
    } }
    AndroidView(factory = { view }, modifier = modifier)
    DisposableEffect(view, owner) {
        var consumed = false
        view.decodeContinuous(object : BarcodeCallback {
            override fun barcodeResult(result: BarcodeResult) {
                if (!consumed && currentText(result.text)) { consumed = true; view.pause() }
            }
        })
        val cameraState = object : CameraPreview.StateListener {
            override fun previewSized() {}
            override fun previewStarted() {}
            override fun previewStopped() {}
            override fun cameraClosed() {}
            override fun cameraError(error: Exception) { currentFailure() }
        }
        view.barcodeView.addStateListener(cameraState)
        fun resume() {
            if (!consumed) runCatching { view.resume() }.onFailure { currentFailure() }
        }
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) resume()
            else if (event == Lifecycle.Event.ON_PAUSE) view.pause()
        }
        owner.lifecycle.addObserver(observer)
        if (owner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) resume()
        onDispose {
            consumed = true
            owner.lifecycle.removeObserver(observer)
            view.barcodeView.stopDecoding()
            view.pause()
        }
    }
}
