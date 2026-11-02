<?php

// Written by Hearth, and overwritten by it: changes here are lost.
// It defines dump() and dd() before the app loads, so a copy of every dump goes to Hearth's Dumps
// page; then the dump prints as it always does. Turn dumps off in Hearth to stop loading this file.

if (!function_exists('__hearth_dump_text')) {
    /** A dump as plain text: VarDumper's CLI form when the app has it, print_r otherwise. */
    function __hearth_dump_text(mixed $var): string
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

    /** Sends one dump to Hearth; gives up at once when Hearth is not listening. */
    function __hearth_dump_send(array $vars, ?array $caller): void
    {
        try {
            $text = $vars === [] ? '(dump without arguments)' : implode("\n", array_map('__hearth_dump_text', $vars));
            if (strlen($text) > 262144) {
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
            ], JSON_INVALID_UTF8_SUBSTITUTE | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
            if ($json === false) {
                return;
            }
            $socket = @stream_socket_client('tcp://127.0.0.1:__HEARTH_PORT__', $errno, $error, 0.05);
            if ($socket === false) {
                return;
            }
            stream_set_timeout($socket, 0, 200000);
            @fwrite($socket, $json . "\n");
            @fclose($socket);
        } catch (\Throwable) {
            // A dump must never break the app.
        }
    }

    /** Prints the values the way the app would without Hearth. */
    function __hearth_dump_print(array $vars): void
    {
        $varDumper = 'Symfony\Component\VarDumper\VarDumper';
        if (class_exists($varDumper)) {
            if (array_key_exists(0, $vars) && count($vars) === 1) {
                $varDumper::dump($vars[0]);
            } else {
                foreach ($vars as $k => $v) {
                    $varDumper::dump($v, is_int($k) ? 1 + $k : $k);
                }
            }
            return;
        }
        foreach ($vars as $v) {
            echo in_array(PHP_SAPI, ['cli', 'phpdbg', 'embed'], true)
                ? print_r($v, true) . "\n"
                : '<pre>' . htmlspecialchars(print_r($v, true)) . '</pre>';
        }
    }
}

if (!function_exists('dump')) {
    function dump(mixed ...$vars): mixed
    {
        __hearth_dump_send($vars, debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0] ?? null);
        __hearth_dump_print($vars);
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
        __hearth_dump_send($vars, debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0] ?? null);
        __hearth_dump_print($vars);
        exit(1);
    }
}
