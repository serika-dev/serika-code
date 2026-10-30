# Authentication

Serika Code uses Serika Accounts. It does not have its own user directory.

Accounts base URL: `https://accounts.serika.dev`.

## What the service actually supports

Verified in `serika-accounts`:

| Call | Role |
| --- | --- |
| `POST /api/auth/login` | Email, password, optional `twoFactorCode`, optional `productId`. Returns `token`, `refreshToken`, and `user`. |
| `POST /api/auth/magic-link` | Sends a sign-in link when the account can use one. The response does not reveal whether the address exists. |
| `POST /api/auth/magic-link/consume` | Redeems the token from the link (`/magic/<token>`). Returns the same session payload. May require `twoFactorCode`. |
| `POST /api/auth/refresh` | Body `{ "refreshToken": "..." }`. Rotates both tokens. |
| `POST /api/auth/logout` | Revokes the session in the `auth_token` cookie. It does not read `x-auth-token`. The client sends `Cookie: auth_token=<jwt>` so only this session is revoked. |
| `GET /api/user/me` | Profile. Header `x-auth-token`. |

The JWT matches the shape the main API already accepts: `user.id`, `userId`, and `username`. Chat requests send it as `x-auth-token`, which is what `middleware/auth.js` reads.

Sign-in sends `productId: "serika-code"` so the session can be told apart in the account's session list.

## Flows that are not available to this client

- There is no OAuth device-code grant.
- Passkey sign-in for a product other than the accounts site returns `USE_HANDOFF`.
- `GET /api/auth/handoff/start` only redirects to a return URL registered for that product. Redeeming the code is `POST /internal/auth/exchange-code`, which requires the service key. `serika-code` is not a registered hand-off product. A native app cannot finish that flow.

Password sign-in and email-link sign-in are the supported native flows. Both return tokens in the JSON body. The password is kept in memory only for the request and is wiped afterwards. It is never written to disk.

A 403 with `code: "TWO_FACTOR_REQUIRED"` asks for the second factor and retries. Other failures (`EMAIL_NOT_VERIFIED`, `AGE_RESTRICTION`, `RATE_LIMITED`, invalid credentials) are shown as the server wrote them.

## Storage

The token, refresh token, user id, username, email, and premium flag are one secret in the OS credential store. Config and SQLite do not contain them. Logout tries the server, then deletes the local secret even if the server is unreachable. Logs run token-shaped strings through a redactor.

If the chat API returns 401, the client refreshes once and retries the call. If refresh fails, the session is expired and the user has to sign in again.
