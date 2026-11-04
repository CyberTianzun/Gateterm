import SwiftUI

struct CloudAccountView: View {
    @Environment(CloudAccountModel.self) private var cloud
    @Environment(\.dismiss) private var dismiss
    @State private var deleteExpanded = false

    var body: some View {
        @Bindable var cloud = cloud
        let enabled = !cloud.busy && !cloud.waitingForBrowser
        NavigationStack {
            Form {
                Section {
                    Text(L.t("cloud_intro")).foregroundStyle(.secondary)
                    if cloud.status != nil && !cloud.notificationsAllowed {
                        HStack {
                            Label(L.t("cloud_notifications_off"), systemImage: "bell.slash").foregroundStyle(.red)
                            Spacer()
                            Button(L.t("cloud_open_notification_settings")) {
                                if let url = URL(string: UIApplication.openNotificationSettingsURLString) { UIApplication.shared.open(url) }
                            }
                        }
                    }
                }
                if let status = cloud.status {
                    Section {
                        LabeledContent(L.t("cloud_email"), value: status.email)
                        LabeledContent(L.t("cloud_relay_region"), value: RelayRegion.from(url: status.url).label)
                        HStack {
                            Text(cloud.push.text)
                                .foregroundStyle(cloud.push.registered || cloud.push == .registering || cloud.push == .disabled ? Color.secondary : Color.red)
                            Spacer()
                            if cloud.push == .registering { ProgressView() }
                            else if !cloud.push.registered && cloud.push != .disabled {
                                Button(L.t("cloud_push_retry")) { Task { await cloud.registerPush(askPermission: true) } }.disabled(!enabled)
                            }
                        }
                        .font(.subheadline)
                    }
                    Section {
                        Button(L.t("cloud_logout")) { cloud.logout() }.disabled(!enabled)
                    }
                    Section {
                        DisclosureGroup(L.t("cloud_delete_title"), isExpanded: $deleteExpanded) {
                            Text(L.t("cloud_delete_hint")).font(.footnote).foregroundStyle(.secondary)
                            SecureField(L.t("cloud_password"), text: $cloud.password).textContentType(.password)
                            HStack {
                                TextField(L.t("cloud_code"), text: $cloud.code).keyboardType(.numberPad).textContentType(.oneTimeCode)
                                Button(L.t("cloud_send_delete_code")) { cloud.sendDeleteCode() }.buttonStyle(.borderless).disabled(!enabled)
                            }
                            Toggle(L.t("cloud_confirm_delete"), isOn: $cloud.confirmDelete)
                            Button(L.t("cloud_delete_account"), role: .destructive) { cloud.deleteAccount() }.disabled(!enabled)
                        }
                        .foregroundStyle(.red)
                    }
                } else {
                    Section {
                        Picker(L.t("cloud_relay_region"), selection: $cloud.region) {
                            ForEach(RelayRegion.allCases) { Text($0.label).tag($0) }
                        }
                        .disabled(!enabled)
                        if !cloud.region.deployed { Text(L.t("cloud_region_pending")).font(.footnote).foregroundStyle(.secondary) }
                    } footer: {
                        Text(L.t("cloud_browser_hint"))
                    }
                    Section {
                        if cloud.waitingForBrowser {
                            HStack { ProgressView(); Text(L.t("cloud_waiting_browser")).foregroundStyle(.secondary) }
                            Button(L.t("cloud_cancel_login"), role: .cancel) { cloud.cancelLogin() }
                        } else {
                            Button { cloud.login() } label: {
                                Text(L.t("cloud_login")).frame(maxWidth: .infinity).bold()
                            }
                            .disabled(!enabled)
                        }
                    }
                }
                if let message = cloud.message {
                    Section { Text(message.text).foregroundStyle(message.ok ? Color.green : Color.red) }
                }
            }
            .navigationTitle(L.t("cloud_title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button(L.t("ios_done")) { dismiss() } }
                if cloud.busy { ToolbarItem(placement: .topBarLeading) { ProgressView() } }
            }
            .onAppear { cloud.refreshStatus() }
        }
    }
}
