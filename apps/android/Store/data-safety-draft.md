# Google Play Data safety — owner review draft

This is an evidence record, not a final legal declaration. Console values saved
to a temporary draft have not been submitted for review.

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

## Current conservative Console draft

- Data collection/sharing: Yes; **Device or other IDs only**.
- Purpose: app functionality for collection and sharing.
- Required for the direct device-transfer feature; not marked ephemeral because
  registered identities can persist locally.
- Encryption of all transmitted data: No, because discovery TXT is plaintext.
- No account creation, federated sign-in, independent security-review badge or
  automatic 90-day deletion claim.
- Optional deletion-request question remains unanswered.

This deliberately conservative draft needs a final policy decision. Its preview
currently produces broad labels equivalent to “device IDs shared with other
companies/organizations” and “data is not transferred over a secure connection.”
Those labels must **not** be represented as an accurate description of file/text
transport: content uses authenticated TLS. Conversely, the absence of a developer
backend must not be used to claim that every network byte is encrypted or that
discovery identifiers do not exist.

Before final Save, review Google's definitions of off-device collection,
user-initiated sharing, end-to-end encryption and applicable exemptions against
these separate data flows. Do not add photos/files/message-body categories merely
because the app transfers them, or omit them merely because it has no server;
the documented definitions and exemptions determine the declaration.

## Primary references checked

- [Google Play Data safety form](https://support.google.com/googleplay/android-developer/answer/10787469?hl=en)
- [Android shared media storage](https://developer.android.com/training/data-storage/shared/media)
- [Android network service discovery](https://developer.android.com/develop/connectivity/wifi/use-nsd)

Public Android privacy policies are separate from this draft:
[English](https://github.com/froiibn/FTA/blob/main/docs/privacy-policy.android.en.md),
[Korean](https://github.com/froiibn/FTA/blob/main/docs/privacy-policy.android.ko.md).
