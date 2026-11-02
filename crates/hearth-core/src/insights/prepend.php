<?php

// Written by Hearth, and overwritten by it: changes here are lost.
// It defines dump() and dd() before the app loads, so a copy of every dump goes to Hearth's Dumps
// page; then the dump prints as it always does. Turn dumps off in Hearth to stop loading this file.
//
// The file sits at .../symfony/var-dumper/Resources/functions/dump.php on purpose: Laravel finds the
// line a dump came from by looking for that file in the call stack, as with Symfony's own dump().

if (!function_exists('hearth_dump_text')) {
    /** A dump as plain text: VarDumper's CLI form when the app has it, print_r otherwise. */
    function hearth_dump_text(mixed $var): string
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

    /** Sends one dump to Hearth; after one failed connect the rest of this run skips it. */
    function hearth_dump_send(array $vars, ?array $caller): void
    {
        static $down = false;
        if ($down) {
            return;
        }
        try {
            $text = $vars === [] ? '(dump without arguments)' : implode("\n", array_map('hearth_dump_text', $vars));
            $truncated = strlen($text) > 262144;
            if ($truncated) {
                $text = substr($text, 0, 262144);
            }
            if (isset($_SERVER['HTTP_HOST'])) {
                $site = preg_replace('/:\d+$/', '', (string) $_SERVER['HTTP_HOST']);
            } else {
                $cwd = getcwd();
                $site = $cwd === false ? null : basename($cwd);
            }
            $json = json_encode([
                'type' => 'dump',
                'site' => $site,
                'file' => $caller['file'] ?? null,
                'line' => $caller['line'] ?? null,
                'sapi' => PHP_SAPI,
                'uri' => isset($_SERVER['REQUEST_URI']) ? (string) $_SERVER['REQUEST_URI'] : null,
                'text' => $text,
                'truncated' => $truncated,
            ], JSON_INVALID_UTF8_SUBSTITUTE | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
            if ($json === false) {
                return;
            }
            // Windows waits out the whole timeout on a closed loopback port.
            $socket = @stream_socket_client('tcp://127.0.0.1:__HEARTH_PORT__', $errno, $error, 0.05);
            if ($socket === false) {
                $down = true;
                return;
            }
            stream_set_timeout($socket, 0, 200000);
            @fwrite($socket, $json . "\n");
            @fclose($socket);
        } catch (\Throwable) {
            // A dump must never break the app.
        }
    }
}

// VarDumper::dump is called right here, not from a helper: Symfony's source link takes the first
// frame without a class after it, which then is the app's own call to dump() or dd().
if (!function_exists('dump')) {
    function dump(mixed ...$vars): mixed
    {
        hearth_dump_send($vars, debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0] ?? null);
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
        hearth_dump_send($vars, debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0] ?? null);
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
