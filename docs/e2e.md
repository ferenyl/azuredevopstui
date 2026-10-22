# End-to-end tests

`src/app/e2e/` runs the real `App` and `AdoClient` against a local mock Azure DevOps API ([wiremock](https://crates.io/crates/wiremock)) serving committed JSON fixtures. Tests press keys, let the app handle the API responses and assert on app state or the screen rendered to ratatui's `TestBackend`. They are offline and deterministic, and use an in-memory PAT and a temporary config file.

They run with the other tests:

```sh
cargo test        # everything
cargo test e2e    # only the end-to-end tests
```

## Writing a test

Tests live in `src/app/e2e/flows.rs`. `Harness::new()` starts the mock and an app signed in with a PAT; `connect()` runs setup and the first load.

```rust
#[tokio::test]
async fn server_error_shows_error_and_keeps_other_panels() {
    let mut harness = Harness::new().await;
    harness.ado.fail("GET", BUILDS, 500).await; // overrides go before connect()

    harness.connect().await;

    harness.wait_for(|app| app.error.is_some()).await;
    harness.wait_for_text("Add order filter").await;
}
```

- `press`, `type_text`: send keys.
- `wait_for(|app| ...)`, `wait_for_text("...")`: handle API responses until the condition holds (fails after 5 s with the screen).
- `screen()`: the rendered screen as text.
- `ado.fail(method, path, status)`: answer an endpoint with an error instead of its fixture.
- `ado.fail_once(method, path, response)`: answer only the first request with `response` (e.g. a 429 with `Retry-After: 0`), then serve the fixture.
- `ado.expect(mock, response, times)`: answer and assert how often a request arrives (a count or a range), e.g. a `PATCH` with a given body. `ResponseTemplate::set_delay` makes a response slow, to test requests that overlap.
- `ado.received(method, path_suffix)`: how many requests have reached a path ending in the suffix.
- `ado.requests()`: every request so far as `METHOD /path`, e.g. to count what one refresh sends.

## Fixtures

Each file in `src/app/e2e/fixtures/` answers one endpoint for organization `contoso`, project `Fabrikam` and team `Web`; the signed-in user is `me-id`. The data is made up.

```json
{
  "method": "POST",
  "path": "/contoso/Fabrikam/Web/_apis/wit/wiql",
  "bodyContains": "[System.AssignedTo] = @Me",
  "body": { "workItems": [{ "id": 301 }, { "id": 302 }] }
}
```

| Field | Meaning |
| --- | --- |
| `method`, `path` | Required. `path` is matched exactly, without the query. |
| `query` | Optional object; every pair must be in the query string. |
| `bodyContains` | Optional; the request body must contain this text. |
| `status` | Optional, default 200. |
| `body` | JSON response. |

Fixtures with `query` or `bodyContains` win over plain ones on the same path, and `ado.fail`/`ado.expect` win over both.

A request no fixture answers fails the test, and its method and URL are printed as `no fixture for ...`. To cover a new endpoint, add a file for it; the URL shapes are in `src/api/*.rs`.
