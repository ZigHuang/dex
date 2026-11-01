# OpenAI Dex Python SDK

Build Python applications that start Dex threads, run turns, stream progress,
and control workspace access.

## Install

Install the SDK:

```bash
pip install openai-dex
```

## Quickstart

The SDK reuses your existing Dex authentication when one is already
available:

```python
from openai_dex import Dex

with Dex() as dex:
    thread = dex.thread_start()
    result = thread.run("Explain this repository in three bullets.")
    print(result.final_response)
```

`thread.run(...)` returns a `TurnResult` containing the final response,
collected items, and token usage.

## Authentication

Existing Dex authentication is reused automatically. To start ChatGPT
browser login explicitly:

```python
from openai_dex import Dex

with Dex() as dex:
    login = dex.login_chatgpt()
    print(login.auth_url)
    print(login.wait().success)
```

For device-code login:

```python
with Dex() as dex:
    login = dex.login_chatgpt_device_code()
    print(login.verification_url, login.user_code)
    login.wait()
```

For API-key login:

```python
with Dex() as dex:
    dex.login_api_key("sk-...")
```

## Built-In Help

Use Python's standard `help(openai_dex)`, `help(Dex)`, or
`python -m pydoc openai_dex` documentation tools.

## Documentation

- [Getting started](https://github.com/openai/dex/blob/main/sdk/python/docs/getting-started.md)
- [API reference](https://github.com/openai/dex/blob/main/sdk/python/docs/api-reference.md)
- [FAQ](https://github.com/openai/dex/blob/main/sdk/python/docs/faq.md)
- [Examples](https://github.com/openai/dex/blob/main/sdk/python/examples/README.md)

The package is licensed under the
[repository Apache License 2.0](https://github.com/openai/dex/blob/main/LICENSE).
