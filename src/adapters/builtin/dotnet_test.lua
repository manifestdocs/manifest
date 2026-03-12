-- Adapter for `dotnet test` output (C#/F# with xUnit, NUnit, MSTest).
--
-- Parses the standard `dotnet test` verbose output format:
--   Passed  MyNamespace.MyTests.TestMethod [5 ms]
--   Failed  MyNamespace.MyTests.FailingTest [10 ms]
--   Skipped MyNamespace.MyTests.SkippedTest
--
-- Also handles the failure detail section.

function parse(output)
    local results = {}
    local failure_messages = {}

    -- Collect failure messages
    -- Format:
    --   Failed MyNamespace.Tests.TestMethod [10 ms]
    --   Error Message:
    --     Assert.Equal() Failure
    --   Stack Trace:
    --     at MyNamespace.Tests.TestMethod() in /path/Tests.cs:line 42
    local in_failure = false
    local current_name = nil
    local current_lines = {}

    for line in output:gmatch("[^\r\n]+") do
        if line:match("^%s*Failed%s+") then
            -- Start of a new failure detail
            if current_name then
                failure_messages[current_name] = table.concat(current_lines, "\n")
            end
            current_name = line:match("^%s*Failed%s+([%w%.]+)")
            current_lines = {}
            in_failure = true
        elseif in_failure then
            if line:match("^%s*Passed%s+") or line:match("^%s*Skipped%s+") or line:match("^%s*$") and #current_lines > 3 then
                -- End of failure detail
                if current_name then
                    failure_messages[current_name] = table.concat(current_lines, "\n")
                end
                current_name = nil
                current_lines = {}
                in_failure = false
            else
                table.insert(current_lines, line)
            end
        end
    end
    if current_name then
        failure_messages[current_name] = table.concat(current_lines, "\n")
    end

    -- Parse test result lines
    for line in output:gmatch("[^\r\n]+") do
        local state, name, dur

        -- Passed test: "  Passed MyNamespace.Tests.Method [5 ms]"
        local p_name, p_dur = line:match("^%s*Passed%s+([%w%.]+)%s+%[(%d+)%s+ms%]")
        if not p_name then
            p_name = line:match("^%s*Passed%s+([%w%.]+)")
        end
        if p_name then
            state = "passed"
            name = p_name
            dur = p_dur
        end

        -- Failed test: "  Failed MyNamespace.Tests.Method [10 ms]"
        if not name then
            local f_name, f_dur = line:match("^%s*Failed%s+([%w%.]+)%s+%[(%d+)%s+ms%]")
            if not f_name then
                f_name = line:match("^%s*Failed%s+([%w%.]+)")
            end
            if f_name then
                state = "failed"
                name = f_name
                dur = f_dur
            end
        end

        -- Skipped test: "  Skipped MyNamespace.Tests.Method"
        if not name then
            local s_name = line:match("^%s*Skipped%s+([%w%.]+)")
            if s_name then
                state = "skipped"
                name = s_name
            end
        end

        if name and state then
            -- Extract suite from namespace (everything before last dot)
            local suite = name:match("^(.+)%.[^%.]+$")

            -- Look for file:line in failure message
            local file = nil
            local test_line = nil
            local msg = failure_messages[name]
            if msg then
                local f, l = msg:match("in ([^:]+):line (%d+)")
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
                entry.duration_ms = tonumber(dur)
            end

            table.insert(results, entry)
        end
    end

    return results
end
