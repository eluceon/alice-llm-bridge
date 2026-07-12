# Registering the skill in Yandex Dialogs

These steps connect a running `bridge-server` deployment to a private Alice
skill on your own account. Field names below match the "Новый навык" form
in the Yandex Dialogs console as of 2026.

1. Go to <https://dialogs.yandex.ru/developer> and create a new skill.

2. Fill in the **О навыке** section:

   - **Название навыка** — the activation phrase, e.g. «мой помощник». This
     is both the display name and what you say to launch the skill.
   - **Активационные имена** — alternate word-forms for the same name, only
     needed if Alice misrecognizes the base phrase. Leave empty otherwise.
   - **Примеры запросов** — leave the default "Запусти навык" / "Запусти
     чат с" entries; they only populate the catalog listing.
   - **Описание** — required free text, e.g. "Yandex Alice skill that
     turns a Yandex Station into a voice interface for large language
     models."
   - **Голос** — any voice; it has no effect on how the backend responds.
   - **Backend** — keep the **Webhook URL** tab selected and set it to:

     ```
     https://<your-domain>/alice/webhook/<WEBHOOK_SECRET>
     ```

     using the domain from `DOMAIN` and the secret from `WEBHOOK_SECRET` in
     your `.env`.
   - **Хранилище** — leave unchecked; conversation state lives in the
     bridge's own Postgres database, not Alice's session storage.
   - **Тип доступа** — select **Приватный**. A private skill never appears
     in the catalog and skips moderation and testing entirely — it just
     needs to be saved to work on the owner's devices.
   - **Категория** — pick whatever fits, e.g. «Общение».
   - **Возрастные ограничения** — leave unchecked unless relevant.
   - **Имя разработчика** — your name.
   - **Email разработчика** — optional; Yandex sends a confirmation link
     to it if set.
   - **Иконка** (required) — PNG or JPEG, 1024×500. Use
     [`docs/assets/skill-icon.png`](assets/skill-icon.png) from this repo,
     or generate your own from `docs/assets/skill-icon.svg`.
   - **Сайт для верификации прав использования бренда** — skip.
   - **Доступные поверхности** — select **Яндекс Станция** (add others
     only if you plan to use the skill on those surfaces too).
   - **Нужно устройство с экраном** — leave unchecked; this is a
     voice-only skill.

3. Save the skill.

4. Open the console's built-in test chat and send any message. The webhook
   request includes your Yandex account's `user_id`; if `allowed_user_ids`
   in `config.toml` is empty, the server logs a warning containing the
   caller's id on every request. Copy that id into `allowed_user_ids`,
   restart the `app` container, and unknown accounts will be refused from
   then on.

5. On the Yandex Station, signed into the same account, say the activation
   phrase from step 2, e.g. «Алиса, запусти навык мой помощник». Private
   skills are immediately available on the owner's devices as soon as
   they're saved — no publication step needed.
