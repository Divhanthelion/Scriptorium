# The study assistant

The assistant answers questions about what the reader attaches: passages in any of the
library's translations, the Hebrew and Greek under the KJV, commentaries, and
cross-references. The reader brings their own model: a key for Anthropic, OpenAI,
Google, DeepSeek, OpenRouter, or Groq, or a server of their own. The app has no AI
service and never sees the questions.

Each request is the instructions, then the context, then the conversation. The
instructions come from `instructions()` in `crates/core/src/context.rs`, and the context
from the same file's builder (see [LIBRARY.md](LIBRARY.md), "Context engine"). Neither
changes between the turns of a conversation, so providers that cache a prompt's start
(DeepSeek, Anthropic, OpenAI, Gemini) charge little for a follow-up question, however
much is attached.

## What the instructions ask

- **Quote exactly.** Anything in quotation marks is word for word as it stands in the
  attached text. Anything left out, however short, is marked with an ellipsis, and any
  word changed is put in [square brackets]. A paraphrase, a summary, or the model's own
  words never go in quotation marks.
- **Give each quotation its source.** Scripture is cited by reference, and by
  translation when there is more than one. A note is cited by its commentator.
- **Write references in full**, as Book chapter:verse (never "v. 37"), so the app can
  link them. Say whose numbering is used where translations differ.
- **Say whose view a note gives**, rather than presenting it as settled.
- **Say what isn't attached.** If the question needs a passage, translation, or
  commentary that isn't there, the answer says so and points to "Change" above the
  conversation. Wording from memory is called that, never presented as exact.
- **Lead with the answer, then the evidence.** Keep it as short as the question allows.
  Separate what the text says from how it has been read, and present the traditions'
  readings fairly.

The instructions name what is attached: each translation with its abbreviation and
year, and each commentary with its author, date, and tradition.

## How it was tuned

The instructions were tuned on DeepSeek V4 (October 2026) with 16 cases
([tests/ai/cases.json](../tests/ai/cases.json)). Each run puts 19 questions to the model
through the dev server's own chat path, with real context. The cases are:

1. a Greek word study (John 11:33–35, with full lexicon entries);
2. Psalm 23 in the KJV, WEB, Douay-Rheims, and Brenton's Septuagint;
3. Malachi 4 and the JPS's numbering;
4. four commentaries on Romans 3:25;
5. Chrysostom and Augustine on "poor in spirit";
6. cross-references for Isaiah 53;
7. three passages at once;
8. the Hebrew בָּרָא;
9. a saying that isn't in the Bible, with nothing attached;
10. a translation that isn't attached;
11. Tobit;
12. Henry and Gill on Melchizedek;
13. a three-question conversation on Romans 8;
14. the whole of Romans with Henry and Gill (484,000 tokens), and a follow-up;
15. Augustine on Psalm 50 in the Douay-Rheims's numbering;
16. "Is Peter the rock?".

`tests/ai/assistant_eval.py` checks each answer automatically: every quotation against
the attached text, and every reference against the library. Each miss was then read by
hand.

| Run | Time per answer | First words after | Answer length | Attached text quoted word for word | References valid | Cost of 19 answers |
|---|---|---|---|---|---|---|
| First instructions, Flash, thinking | 15.8 s | 11.0 s | 641 words | 232 of 278 (83%) | 108 of 108 | $0.132 |
| Second instructions, Flash, thinking | 16.2 s | 12.9 s | 506 words | 221 of 225 (98%) | 133 of 133 | $0.140 |
| **Final instructions, Flash, thinking** | **13.9 s** | **10.4 s** | **521 words** | **224 of 232 (97%)** | **126 of 126** | **$0.134** |
| Final instructions, Flash, no thinking | 4.8 s | 1.4 s | 466 words | 248 of 259 (96%) | 102 of 102 | $0.111 |
| Second instructions, Flash, low effort | 9.9 s | 6.6 s | 475 words | 217 of 227 (96%) | 114 of 114 | $0.124 |
| Second instructions, Pro, thinking | 33.9 s | 30.9 s | 264 words | 114 of 119 (96%) | 69 of 69 | $0.574 |

Costs are at DeepSeek's off-peak prices; peak hours cost twice as much. Each cost is
mostly the one 484,000-token question ($0.074). Its follow-up cost $0.0036, because
484,096 of its tokens came from DeepSeek's cache.

### What reading the misses showed

- **First instructions.** Quotations were often slightly off. Gill's "having" became
  "had" and "of" became "upon", an omission in Chrysostom had no ellipsis, and the
  WEB's John 3:16 was given from memory as "one and only Son" (the WEB has "his only
  born Son"). Some references were written "v. 37", and answers ran long.
- **Final instructions, with thinking.** Nothing quoted from the attached text was
  altered. What the check still counts is:
  - phrases used as terms in quotation marks ("out of nothing");
  - a list number left off ("1.");
  - punctuation the answer normalised ("--" as "—", nested quotation marks);
  - one source typo silently corrected: CrossWire's Treasury of Scripture Knowledge
    prints "iniquitiesof".
- **Without thinking.** Answers are three times faster, but a few quotations were
  altered. Tobit 1:13 came out as "so that he was his purveyor" (the text has "I").
  Fausset's "he pleads" became "will plead", a word went missing from Gill, and
  John 11:37 was quoted from memory without saying so.
- **Pro.** As faithful as Flash with thinking, and more concise, but it takes 30 seconds
  before its first word and costs four times as much.
- **Without the passage.** Answers to case 9 (nothing attached) and case 10 (another
  translation) said their wording was from memory and how to attach the text. The
  wording itself is sometimes wrong (the WEB's John 3:16 again). Attaching the text is
  what makes it exact.

### What the app does with DeepSeek

- **Flash is the default model.** It is as faithful as Pro, at a quarter of the cost and
  twice the speed.
- **"Think first" is on by default**, with DeepSeek's own effort ("high"). Turning it
  off answers in about 5 seconds instead of 14, with the few misquotations described
  above.
- The app sends DeepSeek its own switches: `thinking` (enabled or disabled) and
  `reasoning_effort`. The reasoning streams in above the answer, under "Thinking…" and
  then "Reasoning".

## Running it again

```bash
cargo run --release -p kjv-devserver
python tests/ai/assistant_eval.py run <provider-id> deepseek-flash runs/flash
python tests/ai/assistant_eval.py misses runs/flash
```

The provider id is the one the dev server gave the provider when it was added (Settings,
AI assistant, at http://127.0.0.1:1420). The key stays in the dev server: on Windows in
Credential Manager, elsewhere in memory until the server stops.

## Not yet done

- **Let the model look things up.** An answer about a passage that isn't attached can
  only be from memory. A tool the model could call to fetch a passage, a note, or a
  lexicon entry would make those answers exact too. That needs function calling, and
  each provider does it differently.
- **Concept words in quotation marks** still slip through now and then. They don't
  misquote anything, but they look like quotations.
