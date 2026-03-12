-- Adapter for RSpec output (Ruby).
--
-- Parses RSpec verbose output format (--format documentation):
--   Authentication
--     with valid credentials
--       logs in successfully
--     with invalid credentials
--       returns an error (FAILED - 1)
--
-- Also parses the default progress format:
--   .F.*.
--
-- And the failure details section.

function parse(output)
    local results = {}
    local failure_messages = {}

    -- Collect failure messages from "Failures:" section
    local in_failures = false
    local current_name = nil
    local current_lines = {}

    for line in output:gmatch("[^\r\n]+") do
        if line:match("^Failures:") or line:match("^Failed examples:") then
            if current_name then
                failure_messages[current_name] = table.concat(current_lines, "\n")
            end
            in_failures = true
            current_name = nil
            current_lines = {}
        elseif in_failures then
            if line:match("^Finished in") or line:match("^%d+ examples?") then
                if current_name then
                    failure_messages[current_name] = table.concat(current_lines, "\n")
                end
                in_failures = false
            else
                -- Failure header: "  1) Authentication with invalid credentials returns an error"
                local fname = line:match("^%s+%d+%)%s+(.+)%s*$")
                if fname then
                    if current_name then
                        failure_messages[current_name] = table.concat(current_lines, "\n")
                    end
                    current_name = fname
                    current_lines = {}
                elseif current_name then
                    table.insert(current_lines, line)
                end
            end
        end
    end
    if current_name then
        failure_messages[current_name] = table.concat(current_lines, "\n")
    end

    -- Parse documentation format output
    -- Track nested describe/context blocks by indentation
    local suite_stack = {}

    for line in output:gmatch("[^\r\n]+") do
        -- Skip non-test lines
        if line:match("^Failures:") or line:match("^Pending:") or line:match("^Finished in") then
            break
        end

        -- Detect example lines with status markers
        -- Passed (no marker or green): "    logs in successfully"
        -- Failed: "    returns an error (FAILED - 1)"
        -- Pending: "    handles edge case (PENDING: Not yet implemented)"
        local indent, text = line:match("^(%s%s+)(%S.+)$")
        if indent and text then
            local depth = #indent / 2

            -- Check if this is a failed test
            local test_name_fail, _fail_num = text:match("^(.+)%s+%(FAILED %- %d+%)%s*$")
            -- Check if this is a pending test
            local test_name_pending = text:match("^(.+)%s+%(PENDING:.+%)%s*$")

            if test_name_fail or test_name_pending or (depth >= 1 and not text:match("^#")) then
                -- Build suite from parent contexts
                local suite_parts = {}
                for i = 1, depth - 1 do
                    if suite_stack[i] then
                        table.insert(suite_parts, suite_stack[i])
                    end
                end
                local suite = nil
                if #suite_parts > 0 then
                    suite = table.concat(suite_parts, " ")
                end

                if test_name_fail then
                    local full_name = suite and (suite .. " " .. test_name_fail) or test_name_fail
                    table.insert(results, {
                        name = test_name_fail,
                        suite = suite,
                        state = "failed",
                        message = failure_messages[full_name],
                    })
                elseif test_name_pending then
                    table.insert(results, {
                        name = test_name_pending,
                        suite = suite,
                        state = "skipped",
                    })
                else
                    -- Could be a describe/context block or a passing test
                    -- Describe blocks usually have children (next line is more indented)
                    -- We track it as a suite name for now
                    suite_stack[depth] = text
                    -- Clear deeper entries
                    for i = depth + 1, #suite_stack do
                        suite_stack[i] = nil
                    end
                end
            elseif indent and text and depth >= 1 then
                suite_stack[depth] = text
                for i = depth + 1, #suite_stack do
                    suite_stack[i] = nil
                end
            end
        end
    end

    -- If documentation format found no results, try the summary line
    -- Format: "10 examples, 2 failures, 1 pending"
    if #results == 0 then
        -- Parse rspec -f progress output using the failures section
        for name, msg in pairs(failure_messages) do
            -- Extract file:line from failure message
            local file, test_line = msg:match("# ([^:]+):(%d+)")
            table.insert(results, {
                name = name,
                state = "failed",
                file = file,
                line = test_line and tonumber(test_line),
                message = msg,
            })
        end
    end

    return results
end
