# Google Play Data safety — owner-approved submission preparation

This records the owner's approved answers and their technical basis. Final
Console Save succeeded on 2026-11-03. The owner subsequently instructed release;
these answers are included in production submission 1 with build 8. Google
approval and public availability remain pending.

## Observed data flows

| Data | Actual destination / protection | Retention and control |
| --- | --- | --- |
| LAN discovery UUID, display name, Android OS marker, protocol version, endpoint | NSD/mDNS on the local network; discovery metadata is **not encrypted** and can be observed by other LAN devices | Online discovery expires; registered peer identity is retained locally until removal |
| File/photo bytes and text/URL content | Direct transfer between selected devices over authenticated TLS; no SEIZIA relay, analytics, advertising or content server | Receiver saves locally; first registration requires recipient approval before content transfer |
| Private device identity key | Android Keystore-protected local storage; not a discovery TXT field or application diagnostic | App-private storage; not sent to SEIZIA |
| Explicitly exported received photos | A new MediaStore `Pictures/FTA` copy created only after the user selects Save to gallery | Public copy can survive app deletion; the user's gallery/cloud backup is outside FTA's control |
| Crash/usage analytics and advertising identifiers | No such SDK or backend in the inspected release; no `AD_ID` permission | No developer collection implemented |

The app's private-storage backup exclusion does not control a user's independent
gallery backup. Sending a file to another device is distinct from sending it to
the developer.

## Saved Console answers

- Data collection/sharing: Yes; **Device or other IDs only**.
- Purpose: app functionality for collection. Sharing is unchecked under the
  user-initiated transfer exception below. The owner approved final Save after
  reviewing the exact public security warning; the answers are now submitted
  with the owner's later release authorization.
- Required for the direct device-transfer feature; not marked ephemeral because
  registered identities can persist locally.
- Encryption of all transmitted data: No, because discovery TXT is plaintext.
- No account creation, federated sign-in, independent security-review badge or
  automatic 90-day deletion claim.
- Optional deletion-request question remains unanswered.

The owner approved this conservative classification. Its saved
preview reports no third-party sharing, Device IDs collection and the broad
“data is not transferred over a secure connection” security warning.
Those labels must **not** be represented as an accurate description of file/text
transport: content uses authenticated TLS. Conversely, the absence of a developer
backend must not be used to claim that every network byte is encrypted or that
discovery identifiers do not exist.

Google's definitions of off-device collection, user-initiated sharing,
end-to-end encryption and applicable exemptions were reviewed against these
separate data flows. Do not add photos/files/message-body categories merely
because the app transfers them, or omit them merely because it has no server;
the documented definitions and exemptions determine the declaration.

## Basis for excluding third-party sharing

The saved answers keep the plaintext installation identifier in collection scope
and set **third-party sharing to No**, as approved by the owner. Google defines a third party as
another organization and exempts specific user-initiated transfers. There is no
external organization endpoint/SDK in this release; selecting a receiving peer,
exporting to the gallery and opening/sharing a received item are explicit user
actions. The superseded Yes sharing draft must not be treated as the
recommended final answer merely because it was the most conservative preview.

This is an interpretation of the cited definitions, not a claim that NSD is
secret. Incidental observation of plaintext discovery remains a real exposure
documented in the privacy policy. “All transmitted data encrypted” remains No;
the broad negative security badge must be explained alongside the fact that
actual file/photo/text/URL content is directly TLS-encrypted. The end-to-end
exception is relevant to content because only the two endpoints hold the keys;
it does not cover the plaintext discovery identifier.

## Primary references checked

- [Google Play Data safety form](https://support.google.com/googleplay/android-developer/answer/10787469?hl=en)
- [Android shared media storage](https://developer.android.com/training/data-storage/shared/media)
- [Android network service discovery](https://developer.android.com/develop/connectivity/wifi/use-nsd)

Public Android privacy policies are separate from this draft:
[English](https://github.com/froiibn/FTA/blob/main/docs/privacy-policy.android.en.md),
[Korean](https://github.com/froiibn/FTA/blob/main/docs/privacy-policy.android.ko.md).
