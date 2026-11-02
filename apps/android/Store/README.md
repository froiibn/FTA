# Android Google Play preparation

Status checkpoint: 2026-11-03. The owner explicitly instructed release after the
last UI changes. **Production version 0.1.0 (8) was submitted**, submission ID
**1**, and a fresh Console entry reports **In review**. The Publishing overview
still includes Google's automatic pre-review checks; this is not public
availability or a completed Google approval. Managed publishing is disabled,
so an approved release is configured for automatic publication.

Android source checkpoint: `3296281`; package `app.fta.android`, ARM64, Android
10/API 29+. The uploaded AAB SHA-256 is
`556f3e5399250ecead7e51e737f4eb7a5def0c885974699e93f27ff9605ff66a`.
Artifacts and signature/resource/alignment validation: `output/android-icon-v8`.
The pending production build 7 was replaced; its internal testing draft was not
rolled out. No tester invitation or user-device uninstall was performed.

[Submission 1](https://play.google.com/console/u/1/developers/6290721239636852370/app/4973686468229683954/publishing/submission-activity/1/details)
contains 20 changes: build 8, 10 localized Store listings, 178 country/region
entries and the saved content/privacy/category declarations. The newly captured
Send/photo-source screens replaced **60** old images across all 10 languages and
phone/7-inch/10-inch families. Original capture pixels are preserved; history,
settings, icon and feature graphics retain the same design. AI labeling remains
No label as explicitly selected by the owner. 131 RGB assets passed dimensions,
alpha and size validation; the updated 10-language phone contact sheet was
visually reviewed. Newly visible peers are genuinely discovered test devices.

## Console and signing

- [FTA dashboard](https://play.google.com/console/u/1/developers/6290721239636852370/app/4973686468229683954/app-dashboard)
- [Store draft](https://play.google.com/console/u/1/developers/6290721239636852370/app/4973686468229683954/store-listings/default/edit)
- [Internal release draft](https://play.google.com/console/u/1/developers/6290721239636852370/app/4973686468229683954/tracks/4701575282462432691/releases/1/prepare)

The organization account's new FTA record exists as an App / Free / en-US app.
The owner approved its two creation declarations and disabled automatic
protection. No existing organization's other app was changed.

The release AAB version 8 has been uploaded and processed. Console confirms API
29 minimum, API 37 target, ARM64 and **16 KB memory page support**. This catalog
compatibility result is not testing of every listed device.

Play App Signing is active. Google app-signing certificate SHA-256:
`21:E3:AB:E1:89:56:D0:FE:2D:6D:30:EC:BB:EC:E5:AE:FF:49:E3:6B:85:26:49:F2:0B:6B:C2:51:E3:4E:72:69`.
The separate upload-candidate public certificate is checked into this folder;
its SHA-256 is
`d04f4a06253c0680193dc470188e2c9618c22bb22a70cd7bf026b652de467db1`.
Private upload material is outside the repository with restricted permissions;
its password is protected by macOS Keychain. Never include it in an artifact,
log or commit. Secure independent backup remains an owner responsibility.

The supplied `native-debug-symbols.zip` is attached to version 8 in Console.
It contains a symbol table, not full source-level DWARF. A stripped relink was
compared byte-for-byte with the AAB's actual library before upload. Reproduce a
sidecar without changing the signed app using:

```sh
python3 apps/android/scripts/prepare-native-symbols.py \
  output/android-icon-v8/FTA-0.1.0-8-release.aab \
  --output-dir output/android-play-v7/symbol-check
```

The script fails on an unexpected library set, stale archive or linker drift.
The current app does not use R8/ProGuard, so there is no invented ReTrace map.
No testers were assigned or invited and no Save and release action was taken.
The existing Galaxy debug installation was preserved. Its debug certificate is
different from Google's production signing certificate, so an eventual first
Play installation cannot be assumed to update that development installation in
place. No uninstall, data reset or unsafe key export was performed; any transition
of that test device requires a separate explicit, data-preserving plan.

## Localization and promotional assets

UI resources cover 139 strings and 3 plurals for English, Korean, Japanese,
Simplified/Traditional Chinese, German, French, Spanish, Brazilian Portuguese
and Italian. Unsupported language fallback was exercised in Dutch. Resources
ship together without requiring a language download. Existing preference keys,
device identifiers, filenames and protocol fields are not translated.

`metadata.json` contains all 10 localized store names/descriptions/headlines;
`release-notes.txt` contains all 10 release-note sections. The owner selected
no AI asset label after reviewing the actual capture/composition workflow.
Final Store Save succeeded and the page reports ready to send for review;
the final updated assets were subsequently included in submission 1. After the first save, a fresh release-editor entry
confirmed all 10 release-note languages persisted. Earlier English-only re-entry
must not be mistaken for loss of the local translations or reported as current.

Processed draft assets: **131** — one shared 512×512 icon, ten 1024×500 feature
graphics, forty 1080×1920 phone images and forty each of the 7-inch and 10-inch
tablet images at 1440×2560. Each locale/form factor has Send, photo-source,
History and Settings images. They use actual Android captures and the existing
brand mark with local AppKit composition/system fonts, not generated app UI,
fake online peers, ratings, awards or transfer results.

Actual baseline captures: 124 including four unsupported-language fallback
screens. All 10 languages also have four actual phone captures each with dark
appearance and 130% font scale. Contact sheets have been visually checked;
this is not native-speaker linguistic review.

Artifacts live under `output/android-play-v7/`: `assets/`, `raw/`,
`visual-review/`, release AAB/APK, `release-validation.json`,
`native-symbol-validation.json`, and `SHA256SUMS`.

## Saved declarations and remaining limits

Dashboard initial setup is complete: Data safety was saved after explicit owner
approval. The dashboard now exposes the production release workflow.
Privacy URL, access instructions, no ads, not a government app, no financial
features, no health features, store listing, category/contact and ratings are
saved. Advertising ID is separately saved No. Tools category and owner-approved
public support email `info@seizia.com` are saved. App access explains the second LAN device,
local-network permission, trusted network and recipient's first approval.

The following records the owner's choices and current release limits:

1. Store AI choice is complete: the owner explicitly selected no label; final
   Store Save succeeded. Captures are genuine and no image generator was used;
   copy, translation and composition code were AI-assisted.
2. The owner approved [IARC terms](https://web.iarcservices.com/terms) and rating
   contact `info@seizia.com`. The questionnaire and resulting ratings were saved:
   Korea/Rest of world 3+, ESRB Everyone, PEGI 3, Germany/Brazil all ages,
   with Users Interact. User exchange is Yes; public sharing, reporting and
   moderation are No; interaction can be limited to approved peers. Target
   audience 13–15, 16–17, 18+ is separately saved, not an IARC age rating.
3. [Data safety draft](data-safety-draft.md): conservative Device IDs draft only,
   with explicit plaintext-discovery / encrypted-content distinction. The owner
   approved the exact classification and broad negative encryption warning.
   Final Save succeeded: Device IDs collection, no third-party sharing, not
   all transmitted data encrypted. These answers are included in submission 1.
4. Production country targeting has all **178 current Console country/region
   entries** saved, including Other countries, Korea, China, Japan and the US;
   the production track remains inactive. This is preparation, not rollout or a
   claim of regional legal clearance. Targeting follows a user's Play account
   country, not physical location. A China row does not guarantee a mainland
   device's Google Play access or replace a separate market's rules.
5. Account tax-residency/profile-name warning needs owner documentation. The
   observed warning mentioned possible withholding/delay, not an observed FTA
   upload block, a new registration charge or a stated deadline.

The connected-device declaration appeared during production validation, although
it had been absent from the app-content checklist. Continuous data transfer to
an external device and the public demonstration URL were saved; the blocking
validation error cleared. The recording shows actual NSD connection, first
recipient approval and a subsequent Completed receipt with the receiver at Home.

The owner-approved video and README are public in documentation-only commit
`ff2faf4` (`[skip ci]`). An anonymous HTTP 200 download matched the original
SHA-256. GitHub's blob page does not preview this large file; use the
[public MP4 download](https://raw.githubusercontent.com/froiibn/FTA/main/docs/android-review/connected-device-review.mp4).
No app source or workflow was included in publication.

Production release `8 (0.1.0)` and all 10 release notes were saved and submitted.
The only listed release warning is the absent R8/ProGuard mapping file;
R8/ProGuard is not used. Submission ID 1 confirms all 20 changes and build 8.
Google's automatic pre-review checks remain distinguishable from its full review
and public rollout. The owner authorized and the agent clicked the actual Send
changes for review confirmation; no further submission should be created merely
to refresh this status.

The internal-release preview shows two warnings: no assigned testers and no
R8/ProGuard mapping file. No testers were invited; R8 is not used, so a fabricated
mapping file must not be uploaded. The native-symbol attachment is present.
No blocking error was listed in that internal preview. Production validation
was subsequently completed as described above. Internal Save and release was
not clicked.

## Actual verification and limits

- Release signature and native/ZIP 16 KB alignment, no debuggable flag, no
  AndroidTest classes and all locale/legal resources were verified locally;
  Google independently reports 16 KB compatibility for the processed bundle.
- API 37 debug and separately signed release emulators passed 31 gallery and
  38 JNI/Keystore/native UI checks. Physical Galaxy S23 API 36 retained its
  existing ID/settings/history across the debug-signature v7 update.
- Real isolated Mac-to-Galaxy synthetic PNG/HEIC receipt and explicit gallery
  exports were verified byte-for-byte, including canonical HEIC filename and
  MediaStore publication. User separately reported successful Android/iPhone/
  computer transfers; this is user-performed interoperability evidence.
- Reconstructed native symbols exactly match the signed AAB library after
  stripping. Android bridge/example Clippy and formatting checks passed.
- API 29 runtime, a 16 KB kernel runtime, all manufacturers' battery policies,
  every Android version and native-speaker review remain unverified. Current
  emulators use a 4 KB kernel. No force-stop, lock-screen or OS restriction
  bypass is implemented or promised.

Actual review submission was performed under the owner's explicit release
instruction. Public availability remains pending. No tester invitation, iOS
change, GitHub Actions execution or mobile source push was performed.
