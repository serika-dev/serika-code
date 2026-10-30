# Serika API

Source of truth: `serika-dev-backend` and a live `GET https://api.serika.dev/api/chat/models` (HTTP 200, `{ free, premium }`). `https://chat.serika.dev` is the chat website. It does not serve `/api/chat/models` (HTTP 404). `https://serika.dev/api/chat/models` is also not the API.

Default origins:

- API: `https://api.serika.dev`
- Accounts: `https://accounts.serika.dev`

The client sends `x-auth-token`, `Accept`, `User-Agent: serika-code/<version>`, and `x-desktop-app: true`. The last header is the existing desktop exemption on the process-wide IP limiter in `server.js`. It does not change `usageMeter`. Quotas are enforced inside `POST /api/chat/message` before any tokens are generated.

## Calls this program makes

### `GET /api/chat/models`

Public. Returns `{ "free": [Model], "premium": [Model] }`. Each model has `id`, `name`, `premium`, `isDefault`, `provider`, `usageMultiplier`, `availability`, and sometimes `maxTokens` and `description`. The list is the chat catalog (`SITE = 'serikachat'` in `routes/chat.js`). The client does not ship its own model list. If the catalog changes, `serika models` shows the new rows.

When `model` is unset, the client prefers, in order, a free catalog id that is actually present: `deepseek/deepseek-v4.1-flash`, `mistralai/mistral-small-2603`, `google/gemma-4-31b-it`, `google/gemma-4-26b-a4b-it`. If none of those are in the response, it uses the free model's `isDefault`, then the first free model. The current free default (`sao10k/l3-lunaris-8b`) is a poor coding default, so it is not chosen while a preferred id is listed. Premium models are never selected automatically.

### `GET /api/chat/usage`

Requires `x-auth-token`. The body is:

```json
{
  "totalMessages": 0,
  "dailyMessages": 0,
  "lastReset": null,
  "plan": {}
}
```

`plan` is `usageMeter.getUsageSummary`. When present, the client displays `tier`, `weekly` (`limit`, `used`, `remaining`, `resetsAt`), `session` (5-hour window), `daily`, `byok`, and `resetOffer`. Missing objects are omitted. They are not filled in with guessed numbers.

### `POST /api/chat/message`

This is the only generation endpoint the agent calls. Body:

```json
{
  "message": "latest user or tool text",
  "modelID": "deepseek/deepseek-v4.1-flash",
  "stream": true,
  "temporary": true,
  "conversationHistory": [{ "role": "system", "content": "..." }],
  "webSearch": false,
  "deepResearch": false,
  "mcp": false,
  "imageRequest": false
}
```

`temporary: true` still runs `usageMeter.assertCanChat` and `recordChatUsage`. It does not create a chat.serika.dev session. `mcp: false` turns off the user's server-side connectors. Web search and deep research are left off so a free account is not failed for a Serika+ feature the agent did not ask for.

Before streaming, the route can respond with:

- `401` — missing or bad token
- `403` — model access, ban, or age restriction
- `429` — `UsageLimitError.toJSON()`: `error`, `code`, `limitReached`, `upgradeUrl`, `tier`, `limit`, `used`, `resetsAt`, and sometimes `resetUrl` and `resetPrice`
- `429` — the IP limiter's plain "too many requests" text, when it is not a usage body

Streaming is `text/event-stream`. Events are `data: {json}` separated by a blank line, then `data: [DONE]`. Content chunks look like `{ "content": "...", "reasoning": "..." }`. Failures in the stream look like `{ "error": "...", "details": "..." }`.

The route's system prompt is the Serika chat prompt. The client also sends its own instructions as a `system` turn inside `conversationHistory`, which the route forwards after that prompt. Tool calls are not a separate channel. See below.

### `GET /api/user/me` on accounts

Identity, `isPremium`, `subscriptionStatus`, verification. The allowance name and token windows come from `/api/chat/usage`, not from invented plan math.

## Usage limits

Paid tiers have a weekly token pool and a 5-hour session window (`utils/usageMeter.js`). The free tier has a smaller weekly pool and can only use free models. `assertCanChat` runs before the model call. Token cost is recorded after the reply, including the model's `usageMultiplier` when the provider reports usage. Bring-your-own-key chat is not metered; the usage payload says `byok: true` when that is the case.

The client never retries a 429 against a different provider. It prints the server message, code, used/limit, reset time, and reset link when those fields exist. `upgradeUrl` of `/premium` is shown as the relative path the server sent. An absolute `resetUrl` is shown as-is (`https://accounts.serika.dev/premium?reset=1` in the backend source).

Every model round in a task is its own `POST /api/chat/message`, so each round is checked and recorded. The client does not batch several model calls into one request to avoid the meter.

## Tool calling is not exposed to API clients

`generateAIResponse` in `routes/chat.js` can pass tools to the provider, but only for image generation and the user's saved MCP servers. Those tools run on the server. The SSE events the client receives are text (`content` / `reasoning`), not `tool_calls`.

These other routes were inspected and are not used:

| Route | Why it is not the agent API |
| --- | --- |
| `POST /api/openai/v1/chat/completions` | Does not read a `tools` field. Usage is `recordAPIUsage` on an `sk-` key. A user JWT on this route does not call `assertCanChat`. |
| `POST /api/openai/v1/oauth/chat/completions` | Forwards messages without tools. Checks a boolean premium flag, then increments `messageCount`. It does not apply the weekly or session allowance first. |
| `POST /api/openai/v1/responses` | Same customer-API key path, not the chat meter. |

Local tools are a fenced block the model writes in the assistant text:

````
```serika-tool
{"name":"read_file","arguments":{"path":"src/lib.rs"}}
```
````

The host parses it, runs the tool, and sends the result back as the next user message, which is another metered chat call. If the model does not emit a block, the turn is plain text. This is a compatible client protocol, not a claim that Serika's API implements client-side tool calling.
