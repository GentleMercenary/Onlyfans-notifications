# Settings
### Root Level

The `settings.json` file should have the following structure:

```json
{
  "actions": { ... },
  "reconnect": true,
  "log_level": "info"
}
```

### Actions

The `actions` section is divided into two parts:
- **default**: Specifies the standard behavior for handling different content types.
- **exceptions**: Defines user-specific overrides.

```json
"actions": {
  "default": { ... },
  "exceptions": [ ... ]
}
```

#### Default Actions

The `default` section contains the following sub-sections:
- **notify**: Defines which content types trigger notifications.
- **download**: Specifies which content types should be downloaded.
- **like**: Determines which content types should be automatically liked.

Each of these sections follows the same structure, where values can be:
- `true` or `false`
- A string value (`"all"`, `"none"`)
- An object containing a selection per content type where each has different accepted values:
  - Every content type accepts `true`/`"all"` and `false`/`"none"`
  - `posts` and `messages` accept an object with a `media` key taking any of the following values:
    - `any`: perform the action if the content has any media
    - `thumbnail`: perform the action only if there is a thumbnail
    - `none`: perform the action of there is no media

Example:

```json
"default": {
  "notify": {
    "posts": "all",
    "messages": { "media": "thumbnail" },
    "stories": true,
    "streams": true,
    "notifications": "all"
  },
  "download": {
    "posts": { "media": "any" },
    "messages": { "media": "thumbnail" },
    "stories": false,
  },
  "like": false
}
```

#### Exceptions

The `exceptions` section allows defining user-specific settings that override the default actions. If multiple overlapping exception actions exist for the same user, the first one listed will be used.

Example:

```json
"exceptions": [
  {
    "users": ["user1", "user2"],
    "actions": {
      "notify": {
        "posts": "all",
        "stories": false
      },
    }
  },
  {
    "users": ["user1"],
    "actions": {
      "notify": {
        "posts": "none"
      },
      "download": true
    }
  }
]
``` 

### Reconnect

The `reconnect` field is a boolean value that determines whether the application should attempt to reconnect after certain network errors.

### Log Level

The `log_level` field sets the verbosity of logs. Accepted values are: `"off" | "trace" | "debug" | "info" | "warn" | "error"`