-- Adapter for pytest output.
--
-- Parses pytest verbose output format:
--   tests/test_auth.py::test_login PASSED
--   tests/test_auth.py::test_invalid_password FAILED
--
-- Also parses the short test summary info section for failure messages.

function parse(output)
    local results = {}
    local failure_messages = {}

    -- Collect failure messages from "FAILURES" or "short test summary info" sections
    local in_failures = false
    local current_name = nil
    local current_lines = {}

    for line in output:gmatch("[^\r\n]+") do
        if line:match("^=+ FAILURES =+$") then
            in_failures = true
            current_name = nil
            current_lines = {}
        elseif in_failures then
            if line:match("^=+ short test summary info =+$") or line:match("^=+%s+%d+") then
                -- End of FAILURES section
                if current_name then
                    failure_messages[current_name] = table.concat(current_lines, "\n")
                end
                in_failures = false
            else
                -- Check for failure header: "_____ test_name _____"
                local fname = line:match("^_+ (.+) _+$")
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

    -- Parse test result lines (verbose format)
    -- Lua patterns don't support alternation, so try each status separately
    for line in output:gmatch("[^\r\n]+") do
        -- Format: "tests/test_auth.py::TestClass::test_method PASSED [ 25%]"
        -- or:     "tests/test_auth.py::test_function FAILED [ 50%]"
        local path_and_name, result
        for _, status in ipairs({"PASSED", "FAILED", "SKIPPED", "ERROR", "XFAIL", "XPASS"}) do
            local p = line:match("^(%S+::%S+)%s+" .. status)
            if p then
                path_and_name = p
                result = status
                break
            end
        end

        if path_and_name and result then
            local state
            if result == "PASSED" or result == "XPASS" then
                state = "passed"
            elseif result == "FAILED" then
                state = "failed"
            elseif result == "SKIPPED" or result == "XFAIL" then
                state = "skipped"
            else
                state = "errored"
            end

            -- Split into file and test name
            local file_path, test_name = path_and_name:match("^(.+%.py)::(.+)$")
            if not file_path then
                test_name = path_and_name
            end

            -- Extract suite from class name if present (TestClass::test_method)
            local suite = nil
            if test_name then
                local cls = test_name:match("^(.+)::")
                if cls then
                    suite = cls
                end
            end

            local entry = {
                name = test_name or path_and_name,
                suite = suite,
                state = state,
                file = file_path,
            }

            -- Look up failure message
            if test_name and failure_messages[test_name] then
                entry.message = failure_messages[test_name]
            end

            table.insert(results, entry)
        end
    end

    return results
end
