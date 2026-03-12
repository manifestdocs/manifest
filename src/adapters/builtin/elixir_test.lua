-- Adapter for `mix test` output (Elixir/ExUnit).
--
-- Parses ExUnit output format:
--   ..*.
--
--   1) test login with invalid password (AuthTest)
--      test/auth_test.exs:15
--      Assertion with == failed
--
-- Verbose format (--trace):
--   AuthTest [test/auth_test.exs]
--     * test login with valid credentials (0.1ms) [L#10]
--     * test login with invalid password (0.2ms) [L#15]

function parse(output)
    local results = {}

    -- Try verbose/trace format first
    -- "  * test login with valid credentials (0.1ms) [L#10]"
    local current_suite = nil
    local current_file = nil

    for line in output:gmatch("[^\r\n]+") do
        -- Suite header: "AuthTest [test/auth_test.exs]"
        local suite, file = line:match("^(%w+)%s+%[(.+)%]%s*$")
        if suite then
            current_suite = suite
            current_file = file
        end

        -- Passed test: "  * test login with valid credentials (0.1ms) [L#10]"
        local test_name, dur, test_line = line:match(
            "^%s+%*%s+test (.+)%s+%(([%d%.]+)ms%)%s+%[L#(%d+)%]"
        )
        if test_name then
            table.insert(results, {
                name = "test " .. test_name,
                suite = current_suite,
                state = "passed",
                file = current_file,
                line = tonumber(test_line),
                duration_ms = math.floor(tonumber(dur)),
            })
        end

        -- Excluded/skipped test: "  * test something (excluded) [L#20]"
        local skip_name, skip_line = line:match(
            "^%s+%*%s+test (.+)%s+%(excluded%)%s+%[L#(%d+)%]"
        )
        if skip_name then
            table.insert(results, {
                name = "test " .. skip_name,
                suite = current_suite,
                state = "skipped",
                file = current_file,
                line = tonumber(skip_line),
            })
        end
    end

    -- If trace format found results, return them
    if #results > 0 then
        return results
    end

    -- Parse failure details from default format
    -- "  1) test login with invalid password (AuthTest)"
    -- "     test/auth_test.exs:15"
    local failure_entries = {}
    local current_name = nil
    local current_lines = {}
    local current_suite_name = nil

    for line in output:gmatch("[^\r\n]+") do
        local num, name, suite = line:match("^%s+(%d+)%)%s+test (.+)%s+%((%w+)%)")
        if num and name then
            if current_name then
                table.insert(failure_entries, {
                    name = "test " .. current_name,
                    suite = current_suite_name,
                    lines = current_lines,
                })
            end
            current_name = name
            current_suite_name = suite
            current_lines = {}
        elseif current_name then
            if line:match("^%s*$") and #current_lines > 2 then
                table.insert(failure_entries, {
                    name = "test " .. current_name,
                    suite = current_suite_name,
                    lines = current_lines,
                })
                current_name = nil
                current_lines = {}
            else
                table.insert(current_lines, line)
            end
        end
    end
    if current_name then
        table.insert(failure_entries, {
            name = "test " .. current_name,
            suite = current_suite_name,
            lines = current_lines,
        })
    end

    -- Convert failure entries to results
    for _, entry in ipairs(failure_entries) do
        local file = nil
        local test_line = nil
        local msg_lines = {}

        for _, l in ipairs(entry.lines) do
            local f, ln = l:match("^%s+([%w/%.%-_]+%.exs?):(%d+)")
            if f then
                file = f
                test_line = tonumber(ln)
            else
                local content = l:match("^%s+(.+)")
                if content then
                    table.insert(msg_lines, content)
                end
            end
        end

        table.insert(results, {
            name = entry.name,
            suite = entry.suite,
            state = "failed",
            file = file,
            line = test_line,
            message = #msg_lines > 0 and table.concat(msg_lines, "\n") or nil,
        })
    end

    -- Count passed from summary: "5 tests, 1 failure"
    local total, fail_count = output:match("(%d+) tests?, (%d+) failures?")
    if total and #results > 0 then
        total = tonumber(total)
        fail_count = tonumber(fail_count)
        local passed_count = total - fail_count - (#results - fail_count)
        -- We already have the failures; we can't name individual passed tests
        -- from the dot format, so we don't add synthetic entries
    end

    return results
end
