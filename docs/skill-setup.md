# Registering the skill in Yandex Dialogs

These steps connect a running `bridge-server` deployment to a private Alice
skill on your own account.

1. Go to <https://dialogs.yandex.ru/developer> and create a new skill
   ("Навык в Алисе").

2. Pick a distinctive activation phrase (e.g. «мой помощник») and leave the
   skill in **draft** status. A draft skill only works on the developer
   account's own devices — it never needs to be published or reviewed,
   which is what keeps it private.

3. Under webhook settings, set the endpoint to:

   ```
   https://<your-domain>/alice/webhook/<WEBHOOK_SECRET>
   ```

   using the domain from `DOMAIN` and the secret from `WEBHOOK_SECRET` in
   your `.env`.

4. Leave account linking disabled and NLU intents empty — the backend parses
   control phrases itself and does not rely on Yandex's intent matching.

5. Open the developer console's built-in test chat and send any message.
   The webhook request includes your Yandex account's `user_id`; if
   `allowed_user_ids` in `config.toml` is empty, the server logs a warning
   containing the caller's id on every request. Copy that id into
   `allowed_user_ids`, restart the `app` container, and unknown accounts
   will be refused from then on.

6. On the Yandex Station, signed into the same account, say «Алиса, запусти
   навык мой помощник» (substituting your activation phrase). Draft skills
   are immediately available on the owner's devices without any
   publication step.
