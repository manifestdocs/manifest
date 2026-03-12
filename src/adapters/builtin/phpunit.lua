-- Adapter for PHPUnit output (PHP).
--
-- Parses PHPUnit verbose output format:
--   PHPUnit 10.5.0 by Sebastian Bergmann and contributors.
--
--   Runtime:       PHP 8.3.0
--
--   ..F.S.                                                              6 / 6 (100%)
--
-- Verbose format (-v):
--   ✓ AuthTest::testLogin
--   ✗ AuthTest::testInvalidPassword
--   → AuthTest::testSkipped
--
-- Also: "Tests: 6, Assertions: 10, Failures: 1, Skipped: 1."

function parse(output)
    local results = {}
    local failure_messages = {}

    -- Collect failure details from the failures section
    -- Format:
    --   1) AuthTest::testInvalidPassword
    --   Failed asserting that false is true.
    --
    --   /path/to/tests/AuthTest.php:42
    local current_name = nil
    local current_lines = {}
    local in_failures = false

    for line in output:gmatch("[^\r\n]+") do
        if line:match("^There was %d+ failure") or line:match("^FAILURES!") then
            in_failures = true
        elseif in_failures then
            -- Failure header: "1) AuthTest::testInvalidPassword"
            local fname = line:match("^%d+%)%s+(.+)%s*$")
            if fname then
                if current_name then
                    failure_messages[current_name] = table.concat(current_lines, "\n")
                end
                current_name = fname
                current_lines = {}
            elseif current_name then
                if line:match("^FAILURES!") or line:match("^ERRORS!") or line:match("^OK ") then
                    failure_messages[current_name] = table.concat(current_lines, "\n")
                    current_name = nil
                    in_failures = false
                else
                    table.insert(current_lines, line)
                end
            end
        end
    end
    if current_name then
        failure_messages[current_name] = table.concat(current_lines, "\n")
    end

    -- Try verbose format with unicode markers
    for line in output:gmatch("[^\r\n]+") do
        -- Passed: "✓ AuthTest::testLogin" or "✔ ..."
        local pass_name = line:match("[✓✔]%s+(.+)%s*$")
        if pass_name then
            local suite = pass_name:match("^(.+)::")
            local method = pass_name:match("::(.+)$") or pass_name
            table.insert(results, {
                name = method,
                suite = suite,
                state = "passed",
            })
        end

        -- Failed: "✗ AuthTest::testInvalidPassword" or "✘ ..."
        local fail_name = line:match("[✗✘]%s+(.+)%s*$")
        if fail_name then
            local suite = fail_name:match("^(.+)::")
            local method = fail_name:match("::(.+)$") or fail_name

            -- Extract file:line from failure message
            local file, test_line
            local msg = failure_messages[fail_name]
            if msg then
                local f, l = msg:match("([%w/%.%-_]+%.php):(%d+)")
                if f then
                    file = f
                    test_line = tonumber(l)
                end
            end

            table.insert(results, {
                name = method,
                suite = suite,
                state = "failed",
                file = file,
                line = test_line,
                message = msg,
            })
        end

        -- Skipped/Risky: "→ AuthTest::testSkipped" or "⚠ ..."
        local skip_name = line:match("[→⚠]%s+(.+)%s*$")
        if skip_name then
            local suite = skip_name:match("^(.+)::")
            local method = skip_name:match("::(.+)$") or skip_name
            table.insert(results, {
                name = method,
                suite = suite,
                state = "skipped",
            })
        end
    end

    -- If verbose format found results, return them
    if #results > 0 then
        return results
    end

    -- Fall back to dot-notation format with failure details
    for name, msg in pairs(failure_messages) do
        local suite = name:match("^(.+)::")
        local method = name:match("::(.+)$") or name

        local file, test_line
        local f, l = msg:match("([%w/%.%-_]+%.php):(%d+)")
        if f then
            file = f
            test_line = tonumber(l)
        end

        table.insert(results, {
            name = method,
            suite = suite,
            state = "failed",
            file = file,
            line = test_line,
            message = msg,
        })
    end

    return results
end
