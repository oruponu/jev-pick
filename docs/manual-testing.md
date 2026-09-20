# Manual verification

These checks require an operator's explicit decision to use live Discord and TypeSafe services. They are not part of `cargo test`, CI, or a startup health check. Provider calls may be billable.

Use a private test channel in the configured server and synthetic questions. Record the application revision, model returned, date, and observed behavior. Never record credentials or private inputs.

## Command and response checks

- Confirm startup upserts only `/jev`; other application commands remain intact.
- Run two-, three-, and four-choice questions, with and without context.
- Confirm A through D retain their argument positions and original wording.
- Confirm the result replaces the initial public deferred response, with no extra channel message.
- Ask in English and Japanese; check headings, selected markers, and probability disclaimers.
- Include mixed-language and emoji-only questions and compare with the documented heuristic.
- Submit whitespace-only choices, duplicate choices, and D without C. Confirm a private error and no provider call.
- Reach the same-user cooldown and confirm a private, localized response.
- Use separate users to occupy both evaluation slots; confirm the next request is rejected without queueing.
- Include `@everyone`, user/role mention syntax, Markdown, quotes, and line breaks. Confirm no notifications and no injected formatting.
- Confirm returned percentages remain associated with the correct options and are shown to one decimal place.
- Review logs for safe metadata only.

Do not require a specific model choice to pass. Check that the returned ID belongs to the request, the distribution is valid, and the response completes.

## Failure and resource checks

Automated tests cover provider HTTP failures, deadlines, malformed responses, and admission control without live calls. For end-to-end Discord failure checks, use a dedicated development build and a local mock provider with dummy credentials. The production executable intentionally has no endpoint override.

- After public deferral, force a provider error. Confirm the same public response becomes a safe localized error.
- Force a failed defer. Confirm no provider request is sent and a slot is released.
- Force a failed final edit. Confirm no second provider request is sent.
- Trigger a timeout or cancel an evaluation. Confirm another request can obtain the released slot.
- Test command invocation outside the configured guild, if a stale registration exists. Confirm rejection before provider access.
- Stop with Ctrl+C. Confirm the process disconnects and exits.

Local cancellation does not prove that TypeSafe stopped processing or reversed a charge.

## Release record

State local automated validation and live validation separately. If these checks have not been performed, say so explicitly. A successful build or mock test is not evidence of successful live Discord or Jev operation.