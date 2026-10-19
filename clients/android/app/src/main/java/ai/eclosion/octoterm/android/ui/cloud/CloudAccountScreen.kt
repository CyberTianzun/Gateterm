package ai.eclosion.octoterm.android.ui.cloud

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import ai.eclosion.octoterm.android.R
import ai.eclosion.octoterm.android.cloud.CloudAction
import ai.eclosion.octoterm.android.cloud.CloudForm
import ai.eclosion.octoterm.android.cloud.CloudUiState

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CloudAccountScreen(
    state: CloudUiState,
    pushAvailable: Boolean,
    notificationsEnabled: Boolean,
    onForm: ((CloudForm) -> CloudForm) -> Unit,
    onAction: (CloudAction) -> Unit,
    onOpenNotificationSettings: () -> Unit,
    onBack: () -> Unit,
) {
    val enabled = !state.busy
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(stringResource(R.string.cloud_title))
                        if (state.busy) {
                            CircularProgressIndicator(
                                modifier = Modifier.padding(start = 12.dp).size(18.dp),
                                strokeWidth = 2.dp,
                            )
                        }
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                stringResource(R.string.cloud_intro),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (!pushAvailable) {
                Text(
                    stringResource(R.string.cloud_push_unavailable),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error,
                )
            }
            val status = state.status
            if (status != null && pushAvailable && !notificationsEnabled) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.cloud_notifications_off),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = onOpenNotificationSettings) {
                        Text(stringResource(R.string.cloud_open_notification_settings))
                    }
                }
            }

            if (status == null) {
                SignedOut(state.form, enabled, onForm, onAction)
            } else {
                Text(
                    stringResource(R.string.cloud_logged_in_as, status.email),
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.padding(top = 8.dp),
                )
                Text(
                    status.url,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedButton(onClick = { onAction(CloudAction.Logout) }, enabled = enabled) {
                    Text(stringResource(R.string.cloud_logout))
                }
                DeleteSection(state.form, enabled, onForm, onAction)
            }

            state.message?.let { message ->
                val text = if (message.arg == null) stringResource(message.res) else stringResource(message.res, message.arg)
                Text(
                    text,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (message.ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun SignedOut(
    form: CloudForm,
    enabled: Boolean,
    onForm: ((CloudForm) -> CloudForm) -> Unit,
    onAction: (CloudAction) -> Unit,
) {
    OutlinedTextField(
        value = form.url,
        onValueChange = { v -> onForm { it.copy(url = v) } },
        label = { Text(stringResource(R.string.cloud_url)) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
        modifier = Modifier.fillMaxWidth(),
    )
    OutlinedTextField(
        value = form.email,
        onValueChange = { v -> onForm { it.copy(email = v) } },
        label = { Text(stringResource(R.string.cloud_email)) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
        modifier = Modifier.fillMaxWidth(),
    )
    PasswordField(form, onForm)
    CodeField(form, onForm, supporting = true)
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(onClick = { onAction(CloudAction.Login) }, enabled = enabled) {
            Text(stringResource(R.string.cloud_login))
        }
        OutlinedButton(onClick = { onAction(CloudAction.SendRegisterCode) }, enabled = enabled) {
            Text(stringResource(R.string.cloud_send_register_code))
        }
        OutlinedButton(onClick = { onAction(CloudAction.Register) }, enabled = enabled) {
            Text(stringResource(R.string.cloud_register))
        }
    }
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        TextButton(onClick = { onAction(CloudAction.SendResetCode) }, enabled = enabled) {
            Text(stringResource(R.string.cloud_send_reset_code))
        }
        TextButton(onClick = { onAction(CloudAction.ResetPassword) }, enabled = enabled) {
            Text(stringResource(R.string.cloud_reset_password))
        }
    }
}

@Composable
private fun DeleteSection(
    form: CloudForm,
    enabled: Boolean,
    onForm: ((CloudForm) -> CloudForm) -> Unit,
    onAction: (CloudAction) -> Unit,
) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    HorizontalDivider(Modifier.padding(top = 16.dp))
    TextButton(onClick = { expanded = !expanded }) {
        Text(stringResource(R.string.cloud_delete_title), color = MaterialTheme.colorScheme.error)
    }
    if (!expanded) return
    Text(
        stringResource(R.string.cloud_delete_hint),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    PasswordField(form, onForm)
    Row(verticalAlignment = Alignment.CenterVertically) {
        CodeField(form, onForm, supporting = false, modifier = Modifier.weight(1f))
        TextButton(onClick = { onAction(CloudAction.SendDeleteCode) }, enabled = enabled) {
            Text(stringResource(R.string.cloud_send_delete_code))
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Checkbox(
            checked = form.confirmDelete,
            onCheckedChange = { checked -> onForm { it.copy(confirmDelete = checked) } },
            enabled = enabled,
        )
        Text(stringResource(R.string.cloud_confirm_delete), style = MaterialTheme.typography.bodyMedium)
    }
    Button(
        onClick = { onAction(CloudAction.DeleteAccount) },
        enabled = enabled,
        colors = ButtonDefaults.buttonColors(
            containerColor = MaterialTheme.colorScheme.error,
            contentColor = MaterialTheme.colorScheme.onError,
        ),
    ) {
        Text(stringResource(R.string.cloud_delete_account))
    }
}

@Composable
private fun PasswordField(form: CloudForm, onForm: ((CloudForm) -> CloudForm) -> Unit) {
    OutlinedTextField(
        value = form.password,
        onValueChange = { v -> onForm { it.copy(password = v) } },
        label = { Text(stringResource(R.string.cloud_password)) },
        singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun CodeField(
    form: CloudForm,
    onForm: ((CloudForm) -> CloudForm) -> Unit,
    supporting: Boolean,
    modifier: Modifier = Modifier.fillMaxWidth(),
) {
    OutlinedTextField(
        value = form.code,
        onValueChange = { v -> onForm { it.copy(code = v) } },
        label = { Text(stringResource(R.string.cloud_code)) },
        supportingText = if (supporting) {
            { Text(stringResource(R.string.cloud_code_hint)) }
        } else null,
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = modifier,
    )
}
