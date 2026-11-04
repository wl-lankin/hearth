//! The tools: Herd's MCP tool surface (names, actions, parameters), described for Hearth.

use serde_json::{json, Map, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Number,
    Flag,
}

pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub doc: &'static str,
}

pub struct Action {
    pub name: &'static str,
    /// Parameters this action needs (the schema only requires `action`, as Herd's does).
    pub required: &'static [&'static str],
    pub doc: &'static str,
}

pub struct Tool {
    pub name: &'static str,
    pub summary: &'static str,
    pub actions: &'static [Action],
    pub params: &'static [Param],
}

const fn p(name: &'static str, kind: Kind, doc: &'static str) -> Param {
    Param { name, kind, doc }
}

const fn a(name: &'static str, required: &'static [&'static str], doc: &'static str) -> Action {
    Action { name, required, doc }
}

use Kind::{Flag, Number, Text};

const SITE: Param =
    p("site", Text, "The site's name or a path inside it. Default: the project this server was started in.");

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "site",
        summary: "The sites Hearth serves: details, linking, HTTPS and the PHP version.",
        actions: &[
            a("info", &[], "One site in full: URL, path, HTTPS, PHP and Node versions, its workspaces. Call this first."),
            a("list", &[], "Every site Hearth serves (parked, linked, proxies) with URL, path, HTTPS and PHP version."),
            a("error", &[], "The last exception the site's web requests threw, with its stack trace."),
            a("link", &[], "Serve a project folder (`path`, default: the current one) as `{name}.test`."),
            a("secure", &[], "Serve the site over HTTPS with a locally trusted certificate."),
            a("unsecure", &[], "Serve the site over plain HTTP again."),
            a("isolate", &["phpVersion"], "Serve the site with a specific PHP version instead of the default."),
            a("unisolate", &[], "Serve the site with the default PHP version again."),
            a("studio", &[], "Herd Studio: Hearth has none."),
        ],
        params: &[
            SITE,
            p("path", Text, "link: the project folder. Default: the current one."),
            p("name", Text, "link: the site name (`{name}.test`). Default: the folder's name."),
            p("secure", Flag, "link: serve over HTTPS right away."),
            p("phpVersion", Text, "isolate, link: a PHP version like 8.4."),
        ],
    },
    Tool {
        name: "env",
        summary: "A site's .env file, and which service it uses for the database, queue, cache and broadcasting.",
        actions: &[
            a("get", &[], "Every .env value, or one (`key`)."),
            a("set", &["key", "value"], "Set a value; comments, order and quoting stay."),
            a("unset", &["key"], "Remove a key."),
            a("check", &[], "Compare .env with .env.example: keys missing or extra."),
            a("use_service", &["category", "selection"], "Point a category (database, queue, cache, broadcasting) at a service."),
        ],
        params: &[
            p("key", Text, "The .env key."),
            SITE,
            p("value", Text, "set: the new value."),
            p("category", Text, "use_service: database, queue, cache or broadcasting."),
            p("selection", Text, "use_service: sqlite, sync, database, file, an installed service, or unmanaged."),
        ],
    },
    Tool {
        name: "db",
        summary: "The databases on the server a site's .env points at.",
        actions: &[
            a("connection", &[], "The connection the site's .env describes: driver, host, port, database, user."),
            a("list", &[], "The databases on that server (MySQL, MariaDB, PostgreSQL)."),
            a("create", &[], "Create a database (`name`, default: DB_DATABASE)."),
            a("use", &["name"], "Point the site's DB_DATABASE at a database (`create` to make it first)."),
            a("drop", &["name"], "Delete a database and its data; never the one the site uses."),
        ],
        params: &[SITE, p("name", Text, "The database name."), p("create", Flag, "use: create the database first.")],
    },
    Tool {
        name: "mail",
        summary: "The inbox of Hearth's local mail catcher: every mail the sites send.",
        actions: &[
            a("list", &[], "Mails, newest first: id, date, inbox, subject, sender, recipients, attachments."),
            a("show", &["id"], "One mail as text (default), html, raw or json."),
            a("links", &["id"], "Every http(s) link in a mail."),
            a("wait", &[], "Wait for the next matching mail (`timeout` seconds, default 30, at most 300), then show it."),
            a("clear", &[], "Delete the caught mails (of one `mailbox`, or all)."),
            a("config", &[], "The SMTP host and port to send to, and whether the catcher runs."),
        ],
        params: &[
            p("mailbox", Text, "The inbox: the SMTP user name (MAIL_USERNAME), or the site."),
            p("to", Text, "Only mails to this address (part of it)."),
            p("from", Text, "Only mails from this address (part of it)."),
            p("subject", Text, "Only mails whose subject contains this."),
            p("since", Text, "Only mails since a date, a unix time or a phrase like `10 minutes ago`."),
            p("limit", Number, "list: at most this many (default 50)."),
            p("id", Text, "The mail's id."),
            p("format", Text, "show, wait: text (default), html, raw or json."),
            p("timeout", Number, "wait: seconds (default 30, at most 300)."),
        ],
    },
    Tool {
        name: "insights",
        summary: "What Hearth captures from a site's PHP: dump() output, requests with their queries, and the problems in them (N+1, duplicate and slow queries).",
        actions: &[
            a("list", &[], "Captured dumps and requests, oldest first, each request with its queries."),
            a("analyze", &[], "The problems in the captured requests: N+1, duplicate and slow queries, with file and line. Pass the printed cursor as `after` next time."),
            a("clear", &[], "Delete what was captured."),
            a("capture", &["enabled"], "Switch capturing dumps and requests on or off."),
            a("debug_start", &[], "Herd's debug sessions: Hearth captures all the time instead."),
            a("debug_stop", &[], "Herd's debug sessions: Hearth captures all the time instead."),
        ],
        params: &[
            SITE,
            p("allSites", Flag, "list, analyze, clear: every site, not just one."),
            p("type", Text, "list: dump or request."),
            p("limit", Number, "list, analyze: at most this many (default 50 / 20, at most 200)."),
            p("after", Number, "analyze: only requests after this cursor."),
            p("allRequests", Flag, "analyze: look at every captured request, not only new ones."),
            p("examples", Flag, "analyze: include example queries with their values (may show personal data)."),
            p("minRepeats", Number, "analyze: how often a query must repeat to count as N+1 or duplicate."),
            p("slowMs", Number, "analyze: from how many milliseconds a query is slow."),
            p("enabled", Flag, "capture: on or off."),
        ],
    },
    Tool {
        name: "logs",
        summary: "A site's log files (storage/logs for Laravel), read as entries.",
        actions: &[
            a("read", &[], "Entries, newest first: time, level, message, exception and context. Default file: laravel.log."),
            a("files", &[], "The site's log files with their sizes."),
        ],
        params: &[
            p("file", Text, "read: the log file. Default: laravel.log, else the newest."),
            SITE,
            p("levels", Text, "read: only these levels, comma-separated (error,warning)."),
            p("search", Text, "read: only entries containing this."),
            p("since", Text, "read: only entries since a date or a phrase like `1 hour ago`."),
            p("limit", Number, "read: at most this many (default 20)."),
        ],
    },
    Tool {
        name: "processes",
        summary: "The dev processes Hearth runs for a site in the background, like Vite, a queue worker or Pail.",
        actions: &[
            a("list", &[], "The site's processes and whether they run."),
            a("start", &["process"], "Start a process; it keeps running after the agent finishes."),
            a("stop", &["process"], "Stop a process and everything it started."),
            a("output", &["process"], "The process's latest output (`lines`, default 50, at most 500)."),
        ],
        params: &[
            SITE,
            p("process", Text, "The process's name, as `list` shows it."),
            p("lines", Number, "output: how many lines (default 50, at most 500)."),
        ],
    },
    Tool {
        name: "services",
        summary: "The services Hearth runs: MySQL, MariaDB, PostgreSQL, Redis, Meilisearch, Typesense and more.",
        actions: &[
            a("list", &[], "The services with id, name, type, port, version and whether they run."),
            a("available", &[], "The service types Hearth can install, with versions and default ports."),
            a("install", &["type"], "Install and start a service (`version`, `port`, `name` optional)."),
            a("start", &["service"], "Start a service."),
            a("stop", &["service"], "Stop a service (databases shut down cleanly)."),
        ],
        params: &[
            p("type", Text, "install: mysql, mariadb, postgresql, redis, meilisearch, typesense, ..."),
            p("port", Number, "install: the port. Default: the type's own."),
            p("version", Text, "install: the version line. Default: the newest."),
            p("name", Text, "install: a name for it."),
            p("service", Text, "start, stop: the service's id or name."),
        ],
    },
    Tool {
        name: "workspaces",
        summary: "A branch of a site in its own git worktree and .test domain, for parallel work.",
        actions: &[
            a("list", &[], "The workspaces (of one site, or `allSites`)."),
            a("create", &[], "A new workspace (`name` is the branch, default: a random one); `database` shared, clone or empty."),
            a("delete", &["name"], "Delete a workspace; uncommitted changes are lost, the branch stays."),
            a("merge", &["name"], "Merge the workspace's branch into the branch it came from (`delete` it afterwards)."),
        ],
        params: &[
            SITE,
            p("allSites", Flag, "list: every site's workspaces."),
            p("name", Text, "The workspace's branch or host."),
            p("database", Text, "create: shared (default), clone or empty."),
            p("local", Flag, "create: accepted for Herd's agents; a worktree always starts from the local branch."),
            p("secure", Flag, "create: HTTPS. Default: as the site."),
            p("migrate", Flag, "create: run migrations for an empty database (default true)."),
            p("keepDatabase", Flag, "delete: keep the workspace's own database."),
            p("delete", Flag, "merge: delete the workspace afterwards."),
        ],
    },
    Tool {
        name: "runtime",
        summary: "Hearth itself: status, PHP and Node.js versions.",
        actions: &[
            a("status", &[], "Hearth's version, the default PHP and Node, the web server, mail catcher, collector and services."),
            a("php_versions", &[], "The PHP versions: installed, and available to install."),
            a("php_install", &["version"], "Install a PHP version (or update it to the newest patch)."),
            a("node_versions", &[], "The Node.js versions: installed, and available."),
            a("node_install", &["version"], "Install a Node.js version."),
        ],
        params: &[p("version", Text, "A version like 8.4 or 22.")],
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

impl Tool {
    /// The description agents read: the summary, then each action with what it needs.
    pub fn description(&self) -> String {
        let mut out = format!("{}\n\nActions (pass one as `action`):", self.summary);
        for action in self.actions {
            let needs =
                if action.required.is_empty() { String::new() } else { format!(" ({})", action.required.join(", ")) };
            out.push_str(&format!("\n- {}{needs}: {}", action.name, action.doc));
        }
        out
    }

    pub fn schema(&self) -> Value {
        let mut properties = Map::new();
        let names: Vec<&str> = self.actions.iter().map(|a| a.name).collect();
        properties.insert("action".into(), json!({ "type": "string", "enum": names, "description": "What to do." }));
        for param in self.params {
            let kind = match param.kind {
                Text => "string",
                Number => "integer",
                Flag => "boolean",
            };
            properties.insert(param.name.into(), json!({ "type": kind, "description": param.doc }));
        }
        json!({ "type": "object", "properties": properties, "required": ["action"] })
    }

    /// The action `args` asks for, or the error Herd gives for an unknown one or a missing parameter.
    pub fn action(&self, args: &Value) -> Result<&'static Action, String> {
        let wanted = args.get("action").and_then(Value::as_str).unwrap_or_default();
        let names: Vec<&str> = self.actions.iter().map(|a| a.name).collect();
        let action = self
            .actions
            .iter()
            .find(|a| a.name == wanted)
            .ok_or_else(|| format!("Unknown action \"{wanted}\". Use one of: {}", names.join(", ")))?;
        let missing: Vec<&str> =
            action.required.iter().copied().filter(|p| args.get(*p).is_none_or(Value::is_null)).collect();
        if missing.is_empty() {
            Ok(action)
        } else {
            Err(format!("The {} action needs: {}.", action.name, missing.join(", ")))
        }
    }
}
