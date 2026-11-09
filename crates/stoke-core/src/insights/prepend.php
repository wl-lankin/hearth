<?php

// Written by Stoke, and overwritten by it: changes here are lost. Stoke's php.ini loads it before
// every PHP run. It does two things, each switched in Stoke:
// - dumps: defines dump() and dd(), so a copy of every dump goes to Stoke's Dumps page; then the
//   dump prints as it always does;
// - requests: in a Laravel app, records each request (or short artisan command) with its SQL queries
//   for Stoke's Requests page.
//
// The file sits at .../symfony/var-dumper/Resources/functions/dump.php on purpose: Laravel finds the
// line a dump came from by looking for that file in the call stack, as with Symfony's own dump().
//
// It also starts Xdebug for a site switched to Debug in Stoke: the web server passes XDEBUG_TRIGGER
// as a FastCGI param, which lands in $_SERVER only, while Xdebug looks for it in $_ENV (kept empty).

if (isset($_SERVER['XDEBUG_TRIGGER']) && function_exists('xdebug_connect_to_client') && !xdebug_is_debugger_active()) {
    xdebug_connect_to_client();
}

if (!function_exists('stoke_send_line')) {
    /** Sends one JSON line to Stoke; after one failed connect the rest of this run skips it. */
    function stoke_send_line(array $entry): void
    {
        static $down = false;
        if ($down) {
            return;
        }
        try {
            $json = json_encode($entry, JSON_INVALID_UTF8_SUBSTITUTE | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
            if ($json === false) {
                return;
            }
            // Windows waits out the whole timeout on a closed loopback port.
            $socket = @stream_socket_client('tcp://127.0.0.1:__STOKE_PORT__', $errno, $error, 0.05);
            if ($socket === false) {
                $down = true;
                return;
            }
            stream_set_timeout($socket, 0, 200000);
            @fwrite($socket, $json . "\n");
            @fclose($socket);
        } catch (\Throwable) {
            // Stoke must never break the app.
        }
    }

    /** The host a request was for, or the working folder's name on the command line. */
    function stoke_site(): ?string
    {
        if (isset($_SERVER['HTTP_HOST'])) {
            return preg_replace('/:\d+$/', '', (string) $_SERVER['HTTP_HOST']);
        }
        $cwd = getcwd();
        return $cwd === false ? null : basename($cwd);
    }
}

if (__STOKE_DUMPS__ && !function_exists('stoke_dump_text')) {
    /** A dump as plain text: VarDumper's CLI form when the app has it, print_r otherwise. */
    function stoke_dump_text(mixed $var): string
    {
        $cloner = 'Symfony\Component\VarDumper\Cloner\VarCloner';
        $dumper = 'Symfony\Component\VarDumper\Dumper\CliDumper';
        if (class_exists($cloner) && class_exists($dumper)) {
            $cli = new $dumper();
            $cli->setColors(false);
            return rtrim((string) $cli->dump((new $cloner())->cloneVar($var), true));
        }
        return print_r($var, true);
    }

    /** Sends one dump to Stoke. */
    function stoke_dump_send(array $vars, ?array $caller): void
    {
        try {
            $text = $vars === [] ? '(dump without arguments)' : implode("\n", array_map('stoke_dump_text', $vars));
            $truncated = strlen($text) > 262144;
            if ($truncated) {
                $text = substr($text, 0, 262144);
            }
            stoke_send_line([
                'type' => 'dump',
                'site' => stoke_site(),
                'file' => $caller['file'] ?? null,
                'line' => $caller['line'] ?? null,
                'sapi' => PHP_SAPI,
                'uri' => isset($_SERVER['REQUEST_URI']) ? (string) $_SERVER['REQUEST_URI'] : null,
                'text' => $text,
                'truncated' => $truncated,
            ]);
        } catch (\Throwable) {
            // A dump must never break the app.
        }
    }

    // VarDumper::dump is called right here, not from a helper: Symfony's source link takes the first
    // frame without a class after it, which then is the app's own call to dump() or dd().
    if (!function_exists('dump')) {
        function dump(mixed ...$vars): mixed
        {
            stoke_dump_send($vars, debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0] ?? null);
            if (class_exists('Symfony\Component\VarDumper\VarDumper')) {
                if (array_key_exists(0, $vars) && count($vars) === 1) {
                    \Symfony\Component\VarDumper\VarDumper::dump($vars[0]);
                } else {
                    foreach ($vars as $k => $v) {
                        \Symfony\Component\VarDumper\VarDumper::dump($v, is_int($k) ? 1 + $k : $k);
                    }
                }
            } else {
                foreach ($vars as $v) {
                    echo in_array(PHP_SAPI, ['cli', 'phpdbg', 'embed'], true)
                        ? print_r($v, true) . "\n"
                        : '<pre>' . htmlspecialchars(print_r($v, true)) . '</pre>';
                }
            }
            if (count($vars) > 1) {
                return $vars;
            }
            return $vars === [] ? null : reset($vars);
        }
    }

    if (!function_exists('dd')) {
        function dd(mixed ...$vars): never
        {
            if (!in_array(PHP_SAPI, ['cli', 'phpdbg', 'embed'], true) && !headers_sent()) {
                header('HTTP/1.1 500 Internal Server Error');
            }
            stoke_dump_send($vars, debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0] ?? null);
            if (class_exists('Symfony\Component\VarDumper\VarDumper')) {
                if (array_key_exists(0, $vars) && count($vars) === 1) {
                    \Symfony\Component\VarDumper\VarDumper::dump($vars[0]);
                } else {
                    foreach ($vars as $k => $v) {
                        \Symfony\Component\VarDumper\VarDumper::dump($v, is_int($k) ? 1 + $k : $k);
                    }
                }
            } else {
                foreach ($vars as $v) {
                    echo in_array(PHP_SAPI, ['cli', 'phpdbg', 'embed'], true)
                        ? print_r($v, true) . "\n"
                        : '<pre>' . htmlspecialchars(print_r($v, true)) . '</pre>';
                }
            }
            exit(1);
        }
    }
}

if (!function_exists('stoke_laravel_wanted')) {
    /**
     * Whether this run is a Laravel request or a short artisan command: an `artisan` file next to the
     * script or one folder up (`php -S`, as `artisan serve` runs it, starts in public/). Tests run
     * from vendor/bin and are never recorded.
     */
    function stoke_laravel_wanted(): bool
    {
        if (PHP_SAPI === 'cli-server') {
            $dir = getcwd();
        } else {
            $script = (string) ($_SERVER['SCRIPT_FILENAME'] ?? '');
            $dir = $script === '' ? false : dirname(realpath($script) ?: $script);
        }
        if ($dir === false || !(is_file($dir . '/artisan') || is_file(dirname($dir) . '/artisan'))) {
            return false;
        }
        if (in_array(PHP_SAPI, ['cli', 'phpdbg'], true)) {
            $command = (string) ($_SERVER['argv'][1] ?? '');
            $long = ['serve', 'queue:work', 'queue:listen', 'horizon', 'schedule:work', 'reverb:start', 'pail', 'tinker', 'dev'];
            if (in_array($command, $long, true) || str_starts_with($command, 'octane:') || str_starts_with($command, 'horizon:')) {
                return false;
            }
        }
        return true;
    }
}

if (__STOKE_REQUESTS__ && stoke_laravel_wanted() && !function_exists('stoke_laravel_hook')) {
    /** What this request has collected so far. */
    function &stoke_request_state(): array
    {
        static $state = ['start' => null, 'queries' => [], 'dropped' => 0, 'bytes' => 0];
        if ($state['start'] === null) {
            $state['start'] = (float) ($_SERVER['REQUEST_TIME_FLOAT'] ?? microtime(true));
        }
        return $state;
    }

    /**
     * Called from env(), app() and config(): the first time Laravel's container is there, listens to
     * its queries and sends the request when PHP shuts down. Never throws; without a container it
     * tries again on the next call.
     */
    function stoke_laravel_hook(): void
    {
        static $done = false;
        if ($done) {
            return;
        }
        try {
            $container = 'Illuminate\Container\Container';
            if (!class_exists($container, false)) {
                return;
            }
            // Read the instance without getInstance(), which would make an empty container.
            $app = \Closure::bind(static fn () => static::$instance, null, $container)();
            if (!is_object($app) || !$app->bound('events')) {
                return;
            }
            $done = true;
            stoke_request_state();
            // A Closure: Laravel's dispatcher takes a string listener for a class to resolve.
            $app->make('events')->listen('Illuminate\Database\Events\QueryExecuted', \Closure::fromCallable('stoke_record_query'));
            // Registered from a shutdown function, so it runs after Laravel's own (registered later,
            // while booting): error pages and their queries come first, and so does their status.
            register_shutdown_function(static fn () => register_shutdown_function('stoke_send_request'));
        } catch (\Throwable) {
            $done = true;
        }
    }

    /** A binding as short text. */
    function stoke_binding(mixed $value): string
    {
        $text = match (true) {
            $value === null => 'null',
            is_bool($value) => $value ? 'true' : 'false',
            is_scalar($value) => (string) $value,
            $value instanceof \DateTimeInterface => $value->format('Y-m-d H:i:s'),
            $value instanceof \Stringable => (string) $value,
            default => get_debug_type($value),
        };
        return strlen($text) > 100 ? substr($text, 0, 100) : $text;
    }

    function stoke_record_query(object $event): void
    {
        try {
            $state = &stoke_request_state();
            $sql = (string) ($event->sql ?? '');
            $sql = strlen($sql) > 2000 ? substr($sql, 0, 2000) : $sql;
            $bindings = array_map('stoke_binding', array_slice(array_values((array) ($event->bindings ?? [])), 0, 50));
            // At most 500 queries and about 700 KB of them, so the line stays under the collector's limit.
            $size = strlen($sql) + array_sum(array_map('strlen', $bindings)) + 200;
            if (count($state['queries']) >= 500 || $state['bytes'] + $size > 700000) {
                $state['dropped']++;
                return;
            }
            $state['bytes'] += $size;
            $ms = (float) ($event->time ?? 0);
            $file = null;
            $line = null;
            // The first line of the app itself: not Laravel's (vendor/) and not this file.
            foreach (debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 60) as $frame) {
                $path = $frame['file'] ?? null;
                if ($path === null || $path === __FILE__) {
                    continue;
                }
                $normal = str_replace('\\', '/', $path);
                if (str_contains($normal, '/vendor/')) {
                    continue;
                }
                $file = $path;
                $line = $frame['line'] ?? null;
                break;
            }
            $state['queries'][] = [
                'sql' => $sql,
                'bindings' => $bindings,
                'ms' => $ms,
                'start' => max(0.0, (microtime(true) - $state['start']) * 1000 - $ms),
                'connection' => (string) ($event->connectionName ?? ''),
                'file' => $file,
                'line' => $line,
            ];
        } catch (\Throwable) {
            // Recording must never break a query.
        }
    }

    function stoke_send_request(): void
    {
        try {
            $state = &stoke_request_state();
            $queries = $state['queries'];
            $dropped = $state['dropped'];
            // Out of memory: free the queries and send only the request, or PHP fails again here.
            $error = error_get_last();
            if ($error !== null && str_contains((string) $error['message'], 'Allowed memory size')) {
                $dropped += count($queries);
                $queries = [];
                $state['queries'] = [];
            }
            $cli = in_array(PHP_SAPI, ['cli', 'phpdbg'], true);
            $status = $cli ? null : http_response_code();
            stoke_send_line([
                'type' => 'request',
                'site' => stoke_site(),
                'sapi' => PHP_SAPI,
                'method' => $cli ? null : ($_SERVER['REQUEST_METHOD'] ?? null),
                'uri' => $cli ? null : ($_SERVER['REQUEST_URI'] ?? null),
                'command' => $cli ? implode(' ', array_slice((array) ($_SERVER['argv'] ?? []), 1)) : null,
                'status' => is_int($status) ? $status : null,
                'ms' => (microtime(true) - $state['start']) * 1000,
                'memory' => memory_get_peak_usage(true),
                'queries' => $queries,
                'dropped' => $dropped,
            ]);
        } catch (\Throwable) {
            // Sending must never break the app.
        }
    }

    // Laravel's own helpers, with Laravel's exact bodies (unchanged from Laravel 8 to 12), plus the
    // hook. Laravel defines its own only when none exists yet, so these are the ones the app calls.
    if (!function_exists('env')) {
        function env($key, $default = null)
        {
            stoke_laravel_hook();
            return \Illuminate\Support\Env::get($key, $default);
        }
    }

    if (!function_exists('app')) {
        function app($abstract = null, array $parameters = [])
        {
            stoke_laravel_hook();
            if (is_null($abstract)) {
                return \Illuminate\Container\Container::getInstance();
            }
            return \Illuminate\Container\Container::getInstance()->make($abstract, $parameters);
        }
    }

    if (!function_exists('config')) {
        function config($key = null, $default = null)
        {
            stoke_laravel_hook();
            if (is_null($key)) {
                return app('config');
            }
            if (is_array($key)) {
                return app('config')->set($key);
            }
            return app('config')->get($key, $default);
        }
    }
}
