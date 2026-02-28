-- Adapter for `cargo test` and `cargo nextest` output.
--
-- Parses the standard `cargo test` output format:
--   test module::test_name ... ok
--   test module::other_test ... FAILED
--
-- Also handles the failures section for failure messages.

function parse(output)
    local results = {}
    local failures = {}

    -- Collect failure messages from the "failures:" section
    local in_failures = false
    local current_failure_name = nil
    local current_failure_lines = {}

    for line in output:gmatch("[^\r\n]+") do
        if line:match("^failures:$") or line:match("^failures:%s*$") then
            -- Flush any pending failure
            if current_failure_name then
                failures[current_failure_name] = table.concat(current_failure_lines, "\n")
            end
            in_failures = true
            current_failure_name = nil
            current_failure_lines = {}
        elseif in_failures then
            if line:match("^test result:") or line:match("^%s*$") and current_failure_name == nil then
                -- End of failures section
                if current_failure_name then
                    failures[current_failure_name] = table.concat(current_failure_lines, "\n")
                end
                in_failures = false
            else
                -- Check for failure header: "---- test_name stdout ----"
                local fname = line:match("^%-%-%-%-+ (.+) stdout %-%-%-%-+$")
                if fname then
                    -- Flush previous
                    if current_failure_name then
                        failures[current_failure_name] = table.concat(current_failure_lines, "\n")
                    end
                    current_failure_name = fname
                    current_failure_lines = {}
                elseif current_failure_name then
                    table.insert(current_failure_lines, line)
                end
            end
        end
    end
    -- Flush last failure
    if current_failure_name then
        failures[current_failure_name] = table.concat(current_failure_lines, "\n")
    end

    -- Parse test result lines
    for line in output:gmatch("[^\r\n]+") do
        local name, result = line:match("^test%s+(.+)%s+%.%.%.%s+(%S+)")
        if name and result then
            local state
            if result == "ok" then
                state = "passed"
            elseif result == "FAILED" then
                state = "failed"
            elseif result == "ignored" then
                state = "skipped"
            else
                state = "errored"
            end

            -- Extract suite from module path (everything before last ::)
            local suite = nil
            local last_sep = name:match("^(.+)::")
            if last_sep then
                suite = last_sep
            end

            local entry = {
                name = name,
                suite = suite,
                state = state,
                message = failures[name],
            }

            -- Parse duration if present (cargo nextest format: "... ok 0.12s")
            local dur = line:match("%.%.%.%s+%S+%s+([%d%.]+)s")
            if dur then
                entry.duration_ms = math.floor(tonumber(dur) * 1000)
            end

            table.insert(results, entry)
        end
    end

    return results
end
