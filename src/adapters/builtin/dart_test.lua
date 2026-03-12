-- Adapter for `dart test` and `flutter test` output (Dart).
--
-- Parses the standard dart test output format:
--   00:00 +0: loading test/auth_test.dart
--   00:00 +1: test login with valid credentials
--   00:00 +1 -1: test login with invalid password
--   00:00 +1 -1 ~1: Some tests skipped
--
-- Verbose format (-r expanded):
--   +0: test/auth_test.dart: login with valid credentials
--   +0 -1: test/auth_test.dart: login with invalid password [E]
--     Expected: <true>
--       Actual: <false>

function parse(output)
    local results = {}
    local last_passed = 0
    local last_failed = 0
    local last_skipped = 0

    -- Track test names and their results
    -- Dart test output is incremental — the counters change as tests complete
    local seen_tests = {}

    for line in output:gmatch("[^\r\n]+") do
        -- Parse counter prefix: "00:00 +1 -0 ~0:" or "+1 -0:"
        local passed_str, failed_str, skipped_str, rest
        passed_str, failed_str, skipped_str, rest =
            line:match("^%d+:%d+%s+%+(%d+)%s+%-(%d+)%s+~(%d+):%s*(.*)$")
        if not passed_str then
            passed_str, failed_str, rest =
                line:match("^%d+:%d+%s+%+(%d+)%s+%-(%d+):%s*(.*)$")
            skipped_str = "0"
        end
        if not passed_str then
            passed_str, rest = line:match("^%d+:%d+%s+%+(%d+):%s*(.*)$")
            failed_str = "0"
            skipped_str = "0"
        end

        if passed_str and rest then
            local passed = tonumber(passed_str)
            local failed = tonumber(failed_str)
            local skipped = tonumber(skipped_str)

            -- Skip "loading" and "All tests passed" lines
            if not rest:match("^loading ") and not rest:match("^All tests passed") and rest ~= "" then
                -- Determine state from counter changes
                local state = nil
                if failed > last_failed then
                    state = "failed"
                elseif skipped > last_skipped then
                    state = "skipped"
                elseif passed > last_passed then
                    state = "passed"
                end

                if state and not seen_tests[rest] then
                    seen_tests[rest] = true

                    -- Extract file from expanded format: "test/auth_test.dart: test name"
                    local file, test_name = rest:match("^([%w/%.%-_]+%.dart):%s+(.+)$")
                    if not file then
                        test_name = rest
                    end

                    -- Strip [E] error marker from test name
                    if test_name then
                        test_name = test_name:gsub("%s*%[E%]%s*$", "")
                    end

                    table.insert(results, {
                        name = test_name or rest,
                        state = state,
                        file = file,
                    })
                end
            end

            last_passed = passed
            last_failed = failed
            last_skipped = skipped
        end

        -- Collect failure messages (indented lines after a failed test)
        if #results > 0 and results[#results].state == "failed" then
            local indent_content = line:match("^%s%s+(.+)$")
            if indent_content and not line:match("^%d+:%d+") then
                local prev = results[#results]
                if prev.message then
                    prev.message = prev.message .. "\n" .. indent_content
                else
                    prev.message = indent_content
                end

                -- Extract file:line from failure message
                if not prev.file then
                    local f, l = indent_content:match("([%w/%.%-_]+%.dart):(%d+)")
                    if f then
                        prev.file = f
                        prev.line = tonumber(l)
                    end
                end
            end
        end
    end

    return results
end
