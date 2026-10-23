# Releasing Scriptorium

Builds for every platform run in GitHub Actions (`.github/workflows/release.yml`) when a tag like `v0.1.0` is pushed. The workflow already builds unsigned apps; each account below turns on signing and store delivery for its platform once its secrets are added under **Settings → Secrets and variables → Actions**.

App identifier on every platform: `io.github.divhanthelion.scriptorium`. It is permanent once any store has published the app. Scriptorium is a separate app from KJV Interlinear (`io.github.divhanthelion.kjvinterlinear`), with its own store listings; the two never share an identifier, settings, or keychain entries.

## What only the owner can do

These need your identity, payment, or tax details, so they can't be automated.

### 1. Apple Developer Program (iPhone, iPad, Mac App Store, notarized Mac downloads)

- Enroll at https://developer.apple.com/programs/ ($99/year). Individual enrollment takes about a day.
- In App Store Connect, create an app with bundle ID `io.github.divhanthelion.scriptorium`, category **Reference**, age rating 4+.
- Secrets the workflow uses today (macOS signing and notarization): `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID`. iOS signing and App Store upload are not wired into the workflow yet; when they are, create an **App Store Connect API key** (Users and Access → Integrations) for automated uploads.
- Before the first iOS upload: commit the generated `app/gen/apple` project with `app/PrivacyInfo.xcprivacy` copied into the bundle (Apple rejects binaries without a privacy manifest, ITMS-91053), and answer the export-compliance question (the app's HTTPS uses rustls, not only the OS libraries — answer the questionnaire accordingly or add `ITSAppUsesNonExemptEncryption` once reviewed).

### 2. Google Play (Android), step by step

**Once: the account**

1. Register at https://play.google.com/console as a **personal** account ($25 once). Google verifies your identity (a government ID) and a phone; allow a few days.
2. Verify you have an Android device: on the Play Console home page, open the task "Verify that you have access to an Android mobile device", scan its QR code with your phone (Android 10 or later, not rooted), and tap **Verify** in the Play Console app.
3. **New personal accounts must run a closed test with at least 12 testers, opted in for 14 days in a row, before the app can go to production.** Line up 12 people with Android phones and Google accounts now (a Google Group is the easiest list).

**Once: the upload key** (on your computer, never in the repository)

4. Make the key with the JDK that comes with Android Studio, and choose a strong password when asked (it's used for both the keystore and the key):
   ```powershell
   & "C:\Program Files\Android\Android Studio\jbr\bin\keytool.exe" -genkeypair -v -keystore "$HOME\scriptorium-upload.jks" -keyalg RSA -keysize 2048 -validity 10000 -alias upload
   ```
   Back up the `.jks` file and its password somewhere safe (a password manager). If it's ever lost, Google can reset the upload key, but only through support.
5. Add three secrets at https://github.com/Divhanthelion/Scriptorium/settings/secrets/actions (**New repository secret**):
   - `ANDROID_KEY_BASE64`: the keystore as base64. This puts it on the clipboard to paste:
     ```powershell
     [Convert]::ToBase64String([IO.File]::ReadAllBytes("$HOME\scriptorium-upload.jks")) | Set-Clipboard
     ```
   - `ANDROID_KEY_ALIAS`: `upload`
   - `ANDROID_KEY_PASSWORD`: the password from step 4

**Each release: build it**

6. Set the version in `app/tauri.conf.json` (`"version": "0.1.0"`). Every upload to Play needs a higher one: Tauri turns 0.1.0 into version code 1000, 0.1.1 into 1001, 0.2.0 into 2000.
7. Commit, then tag and push the tag:
   ```bash
   git tag v0.1.0
   git push scriptorium v0.1.0
   ```
8. Wait for **Actions → Release** to finish (about 30 minutes). Download the **android** artifact; inside is `app-universal-release.aab`, signed with the upload key. (The same run makes a *draft* GitHub Release with the desktop installers: publish it or delete it.)

**First release: create the app and fill in its content**

9. In Play Console: **Create app**. Name *Scriptorium*, default language English (United States), **App**, **Free**, and accept the declarations.
10. Under **Policy and programs → App content**, answer each section:
    - **Privacy policy**: https://github.com/Divhanthelion/Scriptorium/blob/main/PRIVACY.md
    - **App access**: "All or some functionality is restricted", with this instruction: *The optional study assistant (the chat button) uses an AI provider and API key of the user's own; Scriptorium has no AI service or accounts. Everything else needs no access.* If review asks to try the assistant, you can give a key with a low spending limit.
    - **Ads**: no ads. **Advertising ID**: not used.
    - **Content rating**: the questionnaire, category *Reference, News, or Educational*. If it asks about generative AI, answer yes: the optional assistant shows answers from an AI service the user sets up. Users don't interact with each other.
    - **Target audience**: 13 and over. Choosing ages under 13 brings in the Families policy.
    - **Data safety**: see "Data safety" below.
    - Government, financial, health, and news declarations: no.
11. Under **Grow → Store presence → Main store listing**:
    - name, short description, and full description (see "Listing text" below);
    - a 512×512 icon and a 1024×500 feature graphic;
    - 2 to 8 phone screenshots. No side may be more than twice the other, so a modern phone's own screenshots (about 9:20) are refused; the **screens-android** artifact of the Device screenshots workflow is 1080×1920;
    - category **Books & Reference**, and a contact email.

**First release: the closed test**

12. **Test and release → Testing → Closed testing → Create track**. Add your testers (the email list or Google Group), then **Create release**:
    - Upload the `.aab`.
    - Accept **Play App Signing**: Google keeps the key that signs what users install; yours only signs uploads.
    - Add release notes, then **Review release → Start rollout**.
13. Send testers the opt-in link from the track's **Testers** tab. They must stay opted in for 14 days.
14. After 14 days: **Dashboard → Apply for production**. Answer the questions about the test (what testers did, what you changed). Google usually decides within seven days.

**Production**

15. Once access is granted: **Production → Create release**. Add the same `.aab` from the library, then **Review → Start rollout**. Review usually takes a few days; the first can take up to a week.
16. Later releases: bump the version (step 6), tag (step 7), and upload the new `.aab` to production (or to testing first).

**Data safety.** The app itself collects nothing. But the optional assistant sends text to the AI service the reader sets up: their question, the passages they attached, and anything it looks up. Google counts data that leaves the device as "collected", even when it goes to a service the user chose. The honest answers:
- **Data collected:** yes. **App activity → Other user-generated content**: optional, for app functionality, not processed ephemerally.
- **Data shared:** no. Sending it to the service the user chose is a user-initiated transfer, which Google doesn't count as sharing.
- **Encrypted in transit:** yes for the cloud services, which all use HTTPS. A server on the user's own network may be plain HTTP; if you want to be strict, answer "no" for that reason.
- **Users can delete their data:** yes. Conversations are kept only on the device, and the reader deletes them in the app (the Conversations list); the developer never receives any. What the AI service keeps is deleted through that service.

**Generative AI.** Google's AI-Generated Content policy asks apps whose central feature is an AI chatbot for a way to report offensive answers without leaving the app. Here the assistant is optional, and each answer's **Report** button opens a prefilled report on GitHub in the browser. If review asks for reporting inside the app, that needs a server to receive reports, which Scriptorium doesn't have; it would be a new decision.

### 3. Microsoft Store (Windows), step by step

An individual Partner Center account is free, and the Store signs the package itself, so no certificate is needed for Store copies. The `Publisher` in `app/windows/msix/AppxManifest.xml` is already your account's.

**Once: the name**

1. Go to https://partner.microsoft.com/dashboard → **Apps and games → New product → MSIX or PWA app**.
2. Reserve **Scriptorium**. Store names are unique; if it's taken, reserve something like *Scriptorium Bible*, and the app's display name can stay *Scriptorium*.
3. Open the new app → **Product management → Product identity**. Copy three values:
   - Package/Identity/Name
   - Package/Identity/Publisher
   - Package/Properties/PublisherDisplayName
4. In `app/windows/msix/AppxManifest.xml`:
   - set `Name="…"` in `<Identity>` to the Name (it replaces the placeholder `Applicant.Scriptorium`);
   - check that `Publisher` and `<PublisherDisplayName>` match exactly.

   Commit and push.

**Each release: build it**

5. Set the version in `app/tauri.conf.json`. The Store package gets it with a fourth number of 0 (0.1.0 → 0.1.0.0), and every submission needs a higher one.
6. Tag and push, as for Android:
   ```bash
   git tag v0.1.0
   git push scriptorium v0.1.0
   ```
7. When **Actions → Release** finishes, download the **microsoft-store** artifact. Inside is `Scriptorium_0.1.0.0_x64.msix`.

**Submit it**

8. In Partner Center, open the app → **Start your submission**, and fill in each section:
   - **Pricing and availability**: all markets, public, **Free**.
   - **Properties**:
     - category **Books & reference**;
     - privacy policy https://github.com/Divhanthelion/Scriptorium/blob/main/PRIVACY.md (required: the app can use the internet);
     - website https://github.com/Divhanthelion/Scriptorium;
     - support contact: https://github.com/Divhanthelion/Scriptorium/issues;
     - **Product declarations**: if one says the product uses generative AI, tick it.
   - **Age ratings**: the IARC questionnaire. The app has no violence, no purchases, and no users talking to each other. If asked about generative AI, say the optional assistant uses an AI service the user sets up.
   - **Packages**: upload the `.msix`, with device family **Desktop** only.
   - **Store listings** (English):
     - description and short description (see "Listing text" below). Store policy 11.16 requires the description to say the app uses live generative AI; that text does;
     - screenshots: at least one, 1366×768 or larger (desktop light, dark, and parallel are good);
     - copyright "© 2025–2026 Divhanthelion".
   - **Submission options**:
     - **Restricted capabilities** (why it needs `runFullTrust`):
       > A desktop application (Rust with the Microsoft Edge WebView2 runtime) packaged as MSIX; runFullTrust is required for a Win32 desktop app.
     - **Notes for certification** (policy 11.16 asks you to note live generative AI here):
       > Everything works offline. The optional study assistant uses live generative AI: answers come from an AI service the user sets up with their own API key (Scriptorium has no AI service or accounts), and nothing else needs it. Each answer has a Report button, which opens a prefilled report to the developer on GitHub.
9. **Submit to the Store**. Certification usually takes up to three business days. If it fails, the report says why; fix it and resubmit.
10. Later releases: bump the version, tag, download the new `.msix`, then **Update** the submission and replace the package.

To build a Store package locally instead: `powershell -File app/windows/msix/pack.ps1 -Exe target/release/scriptorium.exe` (needs the Windows SDK).

**Outside the Store**, Windows SmartScreen warns "Windows protected your PC" on the GitHub installer until it's signed. Azure Trusted Signing (about $10/month; individuals in the US and Canada can validate) can sign them in CI, though that step isn't wired into `release.yml` yet. The Store copy doesn't need it.

### 4. Flathub (Linux)

- Free. Submission is a pull request to https://github.com/flathub/flathub adding a manifest for `io.github.divhanthelion.scriptorium`; Flathub verifies the ID against this GitHub account.

## Release mechanics

- The tag must match the version in `app/tauri.conf.json` and `app/Cargo.toml`; the workflow refuses to build otherwise.
- Unsigned macOS downloads show Gatekeeper's “damaged or incomplete” warning on Apple Silicon until notarization is set up; the Mac App Store build (sandboxed, `.pkg`) is a separate effort not in the pipeline yet.
- Desktop installers from GitHub have no auto-updater; store builds update through their stores.

## Store listing checklist

### Listing text

Ready to paste into both stores. Play's short description must be 80 characters or fewer; this one is 78.

**Short description**

```text
Bible study library: 44 translations, commentaries, Hebrew and Greek. Offline.
```

**Full description**

```text
Scriptorium is a Bible study library that lives on your device. The whole library works offline, with no account, ads, or tracking.

• 44 English translations, from Wycliffe's and Tyndale's through the Geneva Bible, the King James Version, and the Douay-Rheims to the World English Bible and the Berean Standard Bible
• Up to four translations side by side, each in its own verse numbering
• Commentaries from the Church Fathers on: Chrysostom, Augustine, Aquinas's Catena Aurea, Matthew Henry, John Gill, John Wesley, Keil and Delitzsch, Jamieson-Fausset-Brown, and the Tyndale Open Study Notes. Read them on a verse, or straight through a chapter
• Cross-references from the Treasury of Scripture Knowledge and OpenBible.info, with their words
• The Hebrew and Greek behind the King James Version, word by word, with Strong's numbers, grammar, and a lexicon
• Search across every translation and commentary at once

Optional study assistant (generative AI): connect an AI service you choose, or your own server, with your own key, and ask about exactly the passages and sources you pick. Its answers are generated live by that service and can be wrong; each has a Report button. Nothing is sent anywhere unless you set it up and ask.
```

### Checklist

- Privacy policy URL: https://github.com/Divhanthelion/Scriptorium/blob/main/PRIVACY.md
- App icon: 1024×1024 PNG, no transparency. Scriptorium still uses KJV Interlinear's icon (`icon.png`); it needs one of its own.
- Screenshots: iPhone 6.9" (1320×2868), iPad 13" (2064×2752), Android phone (1080×1920; Play refuses anything taller than 1:2), Play feature graphic (1024×500), Mac and Windows desktop
- Content rights: every work and its licence is in NOTICE and in the app under Settings → Licences: 31 translations and 9 commentaries in the public domain (the KJV outside the UK's Crown patent), the rest under Creative Commons (BY, BY-SA, BY-ND, BY-NC-ND; the NonCommercial ones require the app to stay free, with no ads or purchases); STEP Bible's Hebrew and Greek under CC BY 4.0. CCEL asks to be contacted before its texts are republished (a courtesy, not a licence term).
- Apple guideline 4.3 (spam) often catches Bible apps. Lead the description with what is distinctive: a whole study library offline (44 translations, commentaries from the Church Fathers on, cross-references), up to four translations side by side in each one's own verse numbering, search across all of it, and an assistant that reads exactly the passages and sources you choose.
