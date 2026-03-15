---
whoami: amos
name: "@github:tatolab/amos#28"
description: External adapter testing — real OAuth flow end-to-end
adapters:
  github: builtin
---

@github:tatolab/amos#28

This is a manual testing task, not a code change. Set up Google Cloud OAuth credentials, save to `~/.config/amos/adapters/gdoc/credentials.json`, run `pip install google-auth google-auth-oauthlib google-api-python-client`, then test `amos show` on a node with a `@gdoc:` reference. Document any issues with the auth flow.
