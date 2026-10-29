# FTA iOS Privacy Policy
Effective date: October 30, 2026

Operator and contact
FTA (File Transfer Anything) is a local-network transfer app maintained in the public project github.com/froiibn/FTA. Support and privacy inquiries can be submitted at https://github.com/froiibn/FTA/issues. Do not include private file contents, passwords, private keys, or other confidential information in a public issue.

Transfers and device discovery
FTA sends the files, selected photos, captured images, text and URLs you choose directly to another FTA device on the same LAN. It does not use a cloud transfer relay, a developer-operated content collection server, advertising SDKs or usage analytics SDKs. Transfer contents are not automatically sent to the developer.

To discover devices, FTA advertises an app device identifier, display name, operating system and connection service information on the LAN. Other devices on that network can see this discovery information. The recipient must approve device registration once before the first transfer; file names and message bodies are not sent before that approval. Registered devices can subsequently receive content while online without another per-transfer approval. You can unregister devices in Settings.

Photos, camera and file permissions
The system photo picker provides only the photos you select. FTA does not browse or analyze your entire photo library. The camera is used only when you request capture and grant camera permission; captured results are not automatically saved to the photo library. For Files selections, FTA accesses the files you explicitly select and prepares private copies for transfer. If an original photo contains metadata such as its capture location, that metadata may be included in the transferred file.

Local network permission is used for LAN device discovery and transfers. Trusted connections are remembered by the displayed interface and allowed subnet; FTA does not verify a Wi-Fi name or router identity. Sending and receiving stop when the app moves to the background or the screen is locked. If you allow notifications, FTA requests received-result notifications. History remains available if notifications are denied.

Local storage and security
Registered peer information, transfer history, message bodies, settings and transfer copies are stored in the app. Final received files appear inside the FTA folder in the Files app. Exact location labels depend on the device. Transfers use TLS and verify the registered peer key. The private identity key is stored in a device-only Keychain item accessible while unlocked, and is used in app memory during TLS processing.

The private engine state folder containing registrations, history, engine settings and transfer copies is excluded from automatic backup. Final received files and some iOS app preferences may be backed up according to your OS settings. Received files and URLs are not automatically opened. Previewing, sharing, copying and opening links require your action.

Retention and deletion
You can delete transfer records in the app without deleting files already received. During cleanup, terminal completed, partial, failed and cancelled records may be removed according to limits of 30 days, 10,000 records and 100 MiB of message bodies, removing older records first. In-progress transfers and unresolved results are protected from this automatic cleanup. You can move or delete received files in Files.

Transfer copies are cleaned up after successful transfer, selection replacement or clearing, or the next app start. Copies from unsuccessful transfers may remain temporarily for retry. Deleting the app removes its local storage and files, but content already sent to another device and OS backups must be managed separately. iOS may retain Keychain items after app deletion. A new installation does not automatically replace the recipient's device registration approval.

External services and your choices
The support link opens GitHub, whose privacy policy applies there. After you choose to open a received URL or share a file to another app, that website's or app's policies apply. FTA connects to these external services when you choose the relevant action.

Policy updates
This document will be updated if data handling changes. The in-app and public policies are maintained to describe the same actual behavior.
