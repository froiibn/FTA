# Foreground service declaration preparation

Type: `connectedDevice`; proposed use case: continuous data transfer to an
external device over a network. This is an app-to-device LAN connection, not
cloud backup, camera recording, a full-screen notification or a claim to keep
the process alive after a user's explicit stop.

## Evidence and proposed explanation

FTA sends files, photos, text and URLs directly between the user's devices on a
trusted LAN. The user opens FTA, permits local-network access where required,
trusts that LAN and enables sending/receiving. The app displays an ongoing
notification, lets the user return to FTA or explicitly stop the service, and
maintains the network/discovery/transfer engine independently of Activity
recreation. First registration is approved on the recipient before the content
is sent; registered online peers do not require another per-transfer approval.

Deferring the connection makes the device unavailable to the sender while the
user has explicitly enabled receiving. Interrupting it can abort a current
transfer or prevent a subsequent peer transfer. Neither case should be reported
as successful: FTA's final response requires final storage confirmation.

The manifest declares `FOREGROUND_SERVICE`,
`FOREGROUND_SERVICE_CONNECTED_DEVICE` and `CHANGE_WIFI_MULTICAST_STATE`; the
last permission is also used for the real LAN discovery path and satisfies the
documented connected-device prerequisite. Media/file selection uses system
pickers; this service does not turn on camera capture in the background.

Starting and resuming remain subject to OS rules and actual trusted connectivity.
There is no unconditional restart after Force stop or Task Manager Stop, no
battery-optimization exemption forced on the user, and no claim that every
manufacturer will deliver packets through prolonged sleep/Doze. Android 17's
local-network permission is a separate runtime requirement. Notification denial
also changes visibility and must not be disguised as notification delivery.

## Real local demonstration

Artifacts: `output/android-play-v7/fgs-review/`.

- `tablet-raw.mp4`: sending/receiving is disabled and enabled through FTA's own
  Settings, a genuinely discovered second device is selected and a synthetic
  text transfer starts.
- `phone-raw.mp4`: that receiving phone shows the real registration request and
  the user approves it; the first received record reaches Completed. The phone
  then goes Home; the second transfer arrives without another approval.
- `background-notifications.png`: the persistent receiving notification plus
  the two actual received-result notifications.
- `background-real-receipt.png`: both new Received · Completed records.
- `steps.json`: actions/timestamps; no application payload diagnostic log.

Both devices are actual ARM64 API 37 emulators using their real simulated Wi-Fi
and NSD path. The sender runs the separately signed release candidate; the
receiver runs the debug build. No fake peer/online state, fake progress, forged
receipt, private user photo or existing user's peer registration was used.
This demonstrates Home/background receiving, not every OEM's screen-off policy.

Optional composition preserves the complete native recordings and adds labels:

```sh
swift apps/android/scripts/render-fgs-review.swift \
  output/android-play-v7/fgs-review/phone-raw.mp4 \
  output/android-play-v7/fgs-review/tablet-raw.mp4 \
  output/android-play-v7/fgs-review/connected-device-review.mp4
```

No video has been externally hosted. The Console declaration needs an accessible
video URL; choose an owner-approved hosting location before posting. The
foreground-service questionnaire is not yet visible in the observed content
checklist and must be revisited; lack of a displayed form is not a policy
exemption or a completed declaration.

## Official requirements checked

- [Android foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types#connected-device)
- [Google Play foreground-service declarations and video requirements](https://support.google.com/googleplay/android-developer/answer/13392821?hl=en)

Google specifically calls for pairing/connecting inside the app and subsequent
device interaction. A recording of Android system Settings alone is insufficient.
