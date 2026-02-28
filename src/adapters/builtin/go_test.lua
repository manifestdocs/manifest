-- Adapter for `go test -v` output.
--
-- Parses the standard Go test verbose output format:
--   === RUN   TestAuth
--   --- PASS: TestAuth (0.01s)
--   --- FAIL: TestAuth/invalid_password (0.00s)
--       auth_test.go:42: expected error

function parse(output)
    local results = {}
    local failure_messages = {}
    local current_test = nil
    local current_lines = {}

    for line in output:gmatch("[^\r\n]+") do
        -- Track current running test for collecting failure output
        local run_name = line:match("^=== RUN%s+(.+)%s*$")
        if run_name then
            -- Flush previous
            if current_test then
                failure_messages[current_test] = table.concat(current_lines, "\n")
            end
            current_test = run_name
            current_lines = {}
        end

        -- Parse result lines: "--- PASS: TestName (0.01s)"
        local result, name, dur = line:match("^%s*%-%-%-% (PASS|FAIL|SKIP): (.+) %(([%d%.]+)s%)")
        if result and name then
            -- Flush current test output
            if current_test then
                failure_messages[current_test] = table.concat(current_lines, "\n")
                current_test = nil
                current_lines = {}
            end

            local state
            if result == "PASS" then
                state = "passed"
            elseif result == "FAIL" then
                state = "failed"
            else
                state = "skipped"
            end

            -- Extract suite from subtest (TestAuth/subtest -> suite = TestAuth)
            local suite = nil
            local slash_pos = name:find("/")
            if slash_pos then
                suite = name:sub(1, slash_pos - 1)
            end

            -- Look for file:line in failure output
            local file = nil
            local test_line = nil
            local msg = failure_messages[name]
            if msg then
                -- Pattern: "    auth_test.go:42: message"
                local f, l = msg:match("(%S+%.go):(%d+):")
                if f then
                    file = f
                    test_line = tonumber(l)
                end
            end

            local entry = {
                name = name,
                suite = suite,
                state = state,
                file = file,
                line = test_line,
                message = msg,
            }

            if dur then
                entry.duration_ms = math.floor(tonumber(dur) * 1000)
            end

            table.insert(results, entry)
        elseif current_test then
            -- Collect indented output lines for failure messages
            local content = line:match("^%s%s%s%s%s%s%s%s(.+)$") or line:match("^%s%s%s%s(.+)$")
            if content then
                table.insert(current_lines, content)
            end
        end
    end

    return results
end
