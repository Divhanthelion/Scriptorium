# Privacy Policy

**Scriptorium** sends nothing to its developers: it has no servers, accounts, analytics, or tracking. The only data that ever leaves your device is what you choose to send to an AI service you set up yourself (see the study assistant, below).

- The app works entirely offline. The translations, commentaries, cross-references, Hebrew and Greek texts, and lexicons are built into the app.
- It has no accounts, analytics, advertising, or tracking, and it makes no network requests of its own: only to the AI services you set up, as described below.
- Your settings (including saved study contexts), bookmarks, and reading history are stored only on your device, in the app's own storage. The app never sends them anywhere. If your device backs up app data (Google or iCloud backup), they are included in that backup under your platform account's own terms; API keys are excluded from backups.
- Links in the app (on the Licences page, for example) open in your web browser only when you tap them. Those sites have their own privacy policies.

## The optional study assistant

The study assistant is off until you set it up with an AI provider of your choice: a server you run yourself, or a service such as Anthropic, OpenAI, Google, DeepSeek, OpenRouter, or Groq, using your own API key. Scriptorium has no AI service of its own.

- When you send a question, the app sends it, the rest of that conversation, and what you chose to attach (Bible passages, commentary notes, cross-references, Hebrew and Greek words) **directly to the provider you set up**, and nowhere else. The app asks for your permission before the first question goes to each provider. Once providers are set up, the app also refreshes each one's model list from its own server, using its key, when you open the assistant.
- Unless you turn it off (Settings, AI assistant, "Let it look things up"), the assistant can also ask the app for more of its built-in library while it answers: passages, commentary notes, lexicon entries, and search results. The app looks them up on your device and sends them to the same provider, as part of that answer. Each answer lists what it looked up.
- That provider's own terms and privacy policy apply to what you send. Scriptorium's developers never receive or see your questions, the answers, or your API keys.
- API keys are stored in your device's secure credential store (Windows Credential Manager, the macOS or iOS Keychain, the Android Keystore, or the Linux Secret Service), bound to the provider's address, so a saved key is only ever sent to the server it was saved for. If your system has no working credential store, the key is kept in a file in the app's private folder that only your user account can read. Keys in a platform credential store can outlive the app; remove them in the app's AI settings before uninstalling if you want them gone.
- Conversations are saved on your device only, in the app's own storage, so you can reopen them later. You can rename, star, or delete any of them, or clear your history, from the Conversations list. They are never sent anywhere except to the provider you set up, as part of continuing that conversation. Like settings, they are part of your device's normal app-data backup.
- "Report" on an answer opens a report page on GitHub in your browser with the question and answer pre-filled **in the page's address**, so that text reaches GitHub when the page opens. Review and edit it there; nothing is published until you submit the issue.

If this policy ever changes, the new version will be published here with a new date.

Contact: open an issue at https://github.com/Divhanthelion/Scriptorium/issues

_Last updated: 5 October 2026_
