# alice-llm-bridge — Design

A private Yandex Alice skill that lets a family talk to LLMs (DeepSeek and any
OpenAI-compatible provider) by voice through a first-generation Yandex Station.
Backend in Rust.

## Goals

- Voice chat with an LLM through Alice with per-family-member personalization.
- Private: works only for the owner's Yandex account.
- Cost-conscious: configurable context depth, cheap model by default,
  history summarization.
- Multi-provider: any OpenAI-compatible API (DeepSeek, OpenRouter, OpenAI,
  Ollama, …), extensible to native providers later.
- Clean, extensible architecture suitable as a portfolio project.

## Non-goals (v1)

- Web admin UI (config file + voice commands instead).
- Budget hard limits / spending cut-offs (usage is logged and queryable by
  voice; limits can be added later).
- Native Anthropic/Gemini/YandexGPT clients (reachable via OpenRouter).
- Push-initiated interactions (Alice skills cannot start a conversation).

## Key constraint: the webhook deadline

Yandex Dialogs requires the webhook to respond within ~4.5 s, while LLMs often
take longer. Strategy — **hybrid deferred response**:

1. Start the LLM request as a detached tokio task.
2. Race it against a ~2.8 s budget (`tokio::select!`).
3. If the completion arrives in time, answer immediately.
4. Otherwise reply "give me a second, ask me again", keep the task running,
   and stash its result in an in-memory pending store keyed by `user_id`.
   The next utterance (anything: "ну что?", "готово?") returns the finished
   answer, or "still thinking" if the task is not done yet.

Pending results are in-memory only (`DashMap`); they are lost on restart by
design — the user simply asks again.

## Architecture

Cargo workspace, four crates:

```
alice-llm-bridge/
├─ Cargo.toml            # [workspace]
├─ crates/
│  ├─ alice-protocol/    # Yandex Dialogs webhook types (serde only)
│  ├─ llm-providers/     # ChatProvider trait + OpenAI-compatible client
│  ├─ bridge-core/       # domain: profiles, context, modes, commands,
│  │                     # pending answers; depends on traits, not impls
│  └─ bridge-server/     # axum webhook, config loading, Postgres store,
│                        # tracing, wiring (main)
├─ migrations/           # sqlx migrations
├─ docker/
│  ├─ app/Dockerfile
│  └─ postgres/initdb/   # least-privilege role + schema bootstrap
├─ docs/
├─ config.example.toml
├─ compose.yaml          # app + postgres (host nginx handles TLS)
├─ .env.example
└─ docker/postgres/.env.example
```

Dependency directions: `bridge-server` → everything; `bridge-core` →
`llm-providers` (trait) and its own `ConversationStore` trait implemented by
`bridge-server`; `alice-protocol` depends only on serde.

### alice-protocol

Typed request/response models for the Dialogs webhook API, including the
1024-char limits on `text` and `tts`. Round-trip tested against JSON samples
from the official docs. No business logic.

### llm-providers

```rust
#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatCompletion, ProviderError>;
}
```

`ChatRequest`: messages, model, max_tokens, temperature. `ChatCompletion`:
text + token usage. `OpenAiCompatClient` implements the trait for any
`base_url` + API key. `ProviderError` classifies timeout / rate-limit /
auth / server errors so the dialogue layer can phrase them differently.
One retry with backoff on transient network errors, within the time budget.

### bridge-core (domain)

- **Profiles** — loaded from config: name, aliases for voice matching,
  birthday, role (`adult`/`child`), persona text. A default profile applies
  until someone introduces themselves ("это Маша"); the active profile
  persists across sessions until switched.
- **System prompt builder** — base instruction (voice assistant, 1–3
  sentences, no markdown) + current date + active profile card (name, age
  computed from birthday, persona) + family roster with birthdays + active
  mode prompt. `role = "child"` appends a safe-content / simple-language
  block.
- **Context manager** — per-profile history in Postgres. Prompt =
  system + stored summary + last N turns (N configurable per profile and by
  voice). When unsummarized turns exceed 2×N, a background task compresses
  the older part into an updated summary using the cheap model.
- **Model routing** — named presets in config (`fast`, `smart`). Default
  `fast`; "подумай как следует…" upgrades one request, "переключись на
  умную модель" switches until reset.
- **Modes** — config-defined role presets (fairy tale, quiz, homework help,
  translator) with trigger phrases and prompts; active until cancelled.
- **Command parser** — normalizes the utterance and matches control
  commands before any LLM call: introduce ("это <имя>"), forget, set window
  size, switch model, usage stats, who-am-i, modes, help. Everything else
  goes to the LLM.
- **Pending store** — in-memory map of running/finished deferred answers.

### bridge-server

- `POST /alice/webhook/{secret}` — the only business endpoint; `GET /health`.
- Validates `user_id` against the config allowlist (the skill also stays in
  draft status in Yandex Dialogs, which restricts it to the owner's account;
  the allowlist is the second line of defense). Yandex does not sign webhook
  requests, hence the secret path segment.
- Implements `ConversationStore` on sqlx/Postgres.
- Structured logging via `tracing` (JSON in production). API keys come from
  environment variables only and are never logged.

## Data model (Postgres, sqlx migrations)

```sql
messages (
  id BIGSERIAL PRIMARY KEY,
  profile TEXT NOT NULL,
  role TEXT NOT NULL,              -- user | assistant
  content TEXT NOT NULL,
  model TEXT,
  prompt_tokens INT,
  completion_tokens INT,
  cost_micros BIGINT,              -- computed from per-model rates in config
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

summaries (
  profile TEXT PRIMARY KEY,
  content TEXT NOT NULL,
  covers_until_message_id BIGINT NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
```

Usage stats ("сколько потратили") are aggregated from `messages` for
today / this month, per profile and total.

## Configuration (config.toml + env)

```toml
[server]
listen = "0.0.0.0:8080"
webhook_secret = "..."           # or via env
allowed_user_ids = ["..."]

[defaults]
profile = "dima"
context_window = 12
model = "fast"

[providers.deepseek]
base_url = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"

[models.fast]
provider = "deepseek"
model = "deepseek-chat"
max_tokens = 300
input_price_per_mtok = 0.27      # USD, for cost accounting
output_price_per_mtok = 1.10

[models.smart]
provider = "deepseek"
model = "deepseek-reasoner"
max_tokens = 400
input_price_per_mtok = 0.55
output_price_per_mtok = 2.19

[[profiles]]
name = "dima"
aliases = ["дима", "папа"]
birthday = "1985-03-10"
role = "adult"
persona = "Общайся на равных, можно технические детали."

# ... wife, daughter, son

[[modes]]
name = "fairy_tale"
triggers = ["расскажи сказку", "сказка"]
prompt = "Расскажи короткую добрую сказку…"
```

## Error handling

`thiserror` per crate. The user never hears internals: every failure maps to
a human Russian phrase (provider timeout, rate limit, bad key, server error
each get their own), details go to `tracing`. The webhook always returns a
valid Alice response, even on panic (catch layer).

## Voice output constraints

Responses are kept short via `max_tokens` and the system prompt; longer text
is truncated at a sentence boundary to fit the 1024-char limit. TTS field
mirrors text (no SSML in v1).

## Testing

- Unit: command parser (many Russian phrasings), context window assembly,
  cost math, system prompt building (snapshot tests with `insta`).
- `alice-protocol`: serde round-trips on real request/response samples.
- Integration: axum test server + `wiremock` LLM mock, including the
  deferred-answer flow (slow mock → "thinking" reply → follow-up returns the
  answer); Postgres via `sqlx::test`.
- CI (GitHub Actions): `cargo fmt --check`, `clippy -D warnings`, tests,
  Docker image build + Trivy vulnerability scan.

## Deployment

A VPS shared with other services, fronted by the host's own nginx +
certbot rather than a bundled reverse proxy — `app` publishes only on
`127.0.0.1`, so it doesn't compete for ports 80/443. `compose.yaml`: `app`
(multi-stage Rust build, non-root, read-only rootfs, dropped capabilities)
and `postgres` (dedicated `bridge` role and schema, never superuser or
`public`). Secrets are split across two `.env` files — root and
`docker/postgres/` — both gitignored with `.example` variants committed;
`make up` wraps the two-flag `docker compose` invocation both files
require. CI (`ci-cd.yml`) builds, Trivy-scans, and pushes the image to
GHCR on every push to `main`, then deploys over SSH, pinning the exact
digest and rolling back automatically if the post-deploy health check
fails. The skill is registered in Yandex Dialogs pointing at
`https://<domain>/alice/webhook/<secret>` and kept in draft status.

## Future work

- Budget limits with soft/hard cut-offs.
- Native provider clients (YandexGPT for latency, Anthropic, Gemini).
- Web admin (history browser, usage dashboards, profile editing).
- SSML/TTS tuning, sound effects in modes.
