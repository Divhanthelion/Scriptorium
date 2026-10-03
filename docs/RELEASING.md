# Releasing Scriptorium

Builds for every platform run in GitHub Actions (`.github/workflows/release.yml`) when a tag like `v0.1.0` is pushed. The workflow already builds unsigned apps; each account below turns on signing and store delivery for its platform once its secrets are added under **Settings → Secrets and variables → Actions**.

App identifier on every platform: `io.github.divhanthelion.scriptorium`. It is permanent once any store has published the app. Scriptorium is a separate app from KJV Interlinear (`io.github.divhanthelion.kjvinterlinear`), with its own store listings; the two never share an identifier, settings, or keychain entries.

## What only the owner can do

These need your identity, payment, or tax details, so they can't be automated.

### 1. Apple Developer Program (iPhone, iPad, Mac App Store, notarized Mac downloads)

- Enroll at https://developer.apple.com/programs/ ($99/year). Individual enrollment takes about a day.
- In App Store Connect, create an app with bundle ID `io.github.divhanthelion.scriptorium`, category **Reference**, age rating 4+.
- Create an **App Store Connect API key** (Users and Access → Integrations) for automated uploads.
- Secrets the workflow uses: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID`, plus the API key.

### 2. Google Play Console (Android)

- Register at https://play.google.com/console ($25 once, identity verification).
- **New personal accounts must run a closed test with at least 12 testers for 14 continuous days before the app can go to production.** Line up testers early.
- Create an upload key (`keytool -genkeypair -v -keystore upload.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload`), keep it safe, and enroll in Play App Signing.
- Secrets: `ANDROID_KEY_BASE64` (the .jks, base64-encoded), `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.
- The Data safety form: the app collects and shares no data.

### 3. Windows signing and the Microsoft Store

- Without code signing, Windows SmartScreen warns "Windows protected your PC" on the installer from GitHub.
- **Azure Trusted Signing** (about $10/month; individuals in the US and Canada can validate identity) signs the installers in CI.
- **Microsoft Store**: an individual Partner Center account is free. The app is submitted as an **MSIX** ("MSIX or PWA" in Partner Center), which the Store signs itself, so no certificate is needed for Store copies.
  - **Reserve the name "Scriptorium" in Partner Center as a new app** (it can't reuse KJV Interlinear's listing). Its package identity (name, publisher, publisher display name) goes in `app/windows/msix/AppxManifest.xml` and must match Partner Center > Product management > Product identity exactly: the Name there now is a placeholder (`Applicant.Scriptorium`) until the reservation gives the real one; the Publisher is the account's and doesn't change.
  - Every release build produces the package as the `microsoft-store` workflow artifact (`Scriptorium_<version>.0_x64.msix`); upload it under Packages in the submission. To build one locally: `powershell -File app/windows/msix/pack.ps1 -Exe target/release/scriptorium.exe` (needs the Windows SDK).
  - Restricted capability `runFullTrust` (every desktop app has it). Justification for certification: "A desktop application (Rust with the Microsoft Edge WebView2 runtime) packaged as MSIX; runFullTrust is required for a Win32 desktop app."

### 4. Flathub (Linux)

- Free. Submission is a pull request to https://github.com/flathub/flathub adding a manifest for `io.github.divhanthelion.scriptorium`; Flathub verifies the ID against this GitHub account.

## Store listing checklist

- Privacy policy URL: https://github.com/Divhanthelion/Scriptorium/blob/main/PRIVACY.md
- App icon: 1024×1024 PNG, no transparency. Scriptorium still uses KJV Interlinear's icon (`icon.png`); it needs one of its own.
- Screenshots: iPhone 6.9" (1320×2868), iPad 13" (2064×2752), Android phone (1080×1920 or larger), Play feature graphic (1024×500), Mac and Windows desktop
- Content rights: every work and its licence is in NOTICE and in the app under Settings → Licences: 31 translations and 9 commentaries in the public domain (the KJV outside the UK's Crown patent), the rest under Creative Commons (BY, BY-SA, BY-ND, BY-NC-ND; the NonCommercial ones require the app to stay free, with no ads or purchases); STEP Bible's Hebrew and Greek under CC BY 4.0. CCEL asks to be contacted before its texts are republished (a courtesy, not a licence term).
- Apple guideline 4.3 (spam) often catches Bible apps. Lead the description with what is distinctive: a whole study library offline (44 translations, commentaries from the Church Fathers on, cross-references), up to four translations side by side in each one's own verse numbering, search across all of it, and an assistant that reads exactly the passages and sources you choose.
