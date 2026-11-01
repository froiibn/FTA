# Android Google Play preparation

Status checkpoint: 2026-11-02. This folder records preparation, not review
submission or a production/internal-test rollout. Android source checkpoint:
`fb60bfb`; version `0.1.0 (7)`, package `app.fta.android`, ARM64, Android 10/API 29+.

## Console and signing

- [FTA dashboard](https://play.google.com/console/u/1/developers/6290721239636852370/app/4973686468229683954/app-dashboard)
- [Store draft](https://play.google.com/console/u/1/developers/6290721239636852370/app/4973686468229683954/store-listings/default/edit)
- [Internal release draft](https://play.google.com/console/u/1/developers/6290721239636852370/app/4973686468229683954/tracks/4701575282462432691/releases/1/prepare)

The organization account's new FTA record exists as an App / Free / en-US app.
The owner approved its two creation declarations and disabled automatic
protection. No existing organization's other app was changed.

The release AAB version 7 has been uploaded and processed. Console confirms API
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

The supplied `native-debug-symbols.zip` is attached to version 7 in Console.
It contains a symbol table, not full source-level DWARF. A stripped relink was
compared byte-for-byte with the AAB's actual library before upload. Reproduce a
sidecar without changing the signed app using:

```sh
python3 apps/android/scripts/prepare-native-symbols.py \
  output/android-play-v7/FTA-0.1.0-7-release.aab \
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
`release-notes.txt` contains all 10 release-note sections. Console has all 10
store metadata and image sets saved as **drafts**, but final Store save is
pending the AI-asset declaration below. Release notes appear in all 10 in the
editor/preview; a fresh full page entry has shown only English. Remote
re-entry persistence of the other nine must be confirmed before reporting them
as finally saved.

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

## Completed technical/content drafts and remaining decisions

Dashboard initial setup is 6/11 complete: privacy URL, access instructions,
no ads, not a government app, no financial features, no health features.
Advertising ID is separately saved No. Tools category is saved; public support
email is still required. App access correctly explains the second LAN device,
local-network permission, trusted network and recipient's first approval.

The following must not be silently completed on the owner's behalf:

1. Final Store AI-asset choice: Google's current form offers no AI label or
   labeling assets as AI generated/modified. Captures are genuine and no image
   generator was used; copy, translation and composition code were AI-assisted.
   Owner decision is pending before final Store save.
2. IARC service terms: the questionnaire explicitly implies agreement to
   [IARC terms](https://web.iarcservices.com/terms). It has not been completed.
   Proposed target audience is 13–15, 16–17, 18+; it is not saved or a claimed
   obtained rating. Public support/rating contact email is not inferred from a
   private Google login.
3. [Data safety draft](data-safety-draft.md): conservative Device IDs draft only,
   with explicit plaintext-discovery / encrypted-content distinction. Final
   classification and store security badge need owner review.
4. Production country targeting has all **178 current Console country/region
   entries** saved, including Other countries, Korea, China, Japan and the US;
   the production track remains inactive. This is preparation, not rollout or a
   claim of regional legal clearance. Targeting follows a user's Play account
   country, not physical location. A China row does not guarantee a mainland
   device's Google Play access or replace a separate market's rules.
5. Account tax-residency/profile-name warning needs owner documentation. The
   observed warning mentioned possible withholding/delay, not an observed FTA
   upload block, a new registration charge or a stated deadline.

The connected-device foreground-service declaration is not currently shown in
the observed app-content pending/handled lists. This is not an exemption;
recheck the production setup form after initial requirements are complete.
[The declaration draft](foreground-service-draft.md) records an actual
two-emulator NSD connection, first recipient approval and a subsequent Completed
receipt while the receiver was at Home. Its local 66-second composed video and
both original recordings are under `output/android-play-v7/fgs-review/`; complete
native app pixels are preserved and exported frames were visually checked.
No video has been externally posted. A reviewer-accessible video URL still
requires an approved hosting location.

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

No actual review submission, public rollout, tester invitation, iOS change,
GitHub Actions execution or mobile source push was performed.
