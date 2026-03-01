-- Adapter for Jest and Vitest output.
--
-- Parses the standard Jest/Vitest verbose output format:
--   ✓ test name (5 ms)
--   ✕ test name (10 ms)
--   ○ skipped test name
--
-- Also handles the describe block suite names.

function parse(output)
    local results = {}
    local current_suite = nil
    local current_file = nil

    for line in output:gmatch("[^\r\n]+") do
        -- Detect test file header: "PASS src/auth.test.ts" or "FAIL src/auth.test.ts"
        -- Lua patterns don't support alternation, so try each separately
        local file_path = line:match("^%s*PASS%s+(.+)$") or line:match("^%s*FAIL%s+(.+)$")
        if file_path then
            current_file = file_path
            current_suite = nil
        end

        -- Detect describe block (indented suite name)
        local suite_name = line:match("^%s%s+(%S.+%S)%s*$")
        -- Only treat as suite if not a test line
        if suite_name and not line:match("[✓✕○●×∘]") and not line:match("^%s+[✓✕○●×∘]") then
            -- Avoid matching test result lines as suites
            if not line:match("%(.*ms%)") then
                current_suite = suite_name
            end
        end

        -- Parse test results
        -- Passed: "  ✓ test name (5 ms)" or "  √ test name (5 ms)"
        local pass_name, pass_dur = line:match("[✓√]%s+(.-)%s+%((%d+)%s*m?s?%)")
        if not pass_name then
            pass_name = line:match("[✓√]%s+(.+)%s*$")
        end
        if pass_name then
            local entry = {
                name = pass_name,
                suite = current_suite,
                state = "passed",
                file = current_file,
            }
            if pass_dur then
                entry.duration_ms = tonumber(pass_dur)
            end
            table.insert(results, entry)
        end

        -- Failed: "  ✕ test name (10 ms)" or "  × test name (10 ms)"
        local fail_name, fail_dur = line:match("[✕×]%s+(.-)%s+%((%d+)%s*m?s?%)")
        if not fail_name then
            fail_name = line:match("[✕×]%s+(.+)%s*$")
        end
        if fail_name then
            local entry = {
                name = fail_name,
                suite = current_suite,
                state = "failed",
                file = current_file,
            }
            if fail_dur then
                entry.duration_ms = tonumber(fail_dur)
            end
            table.insert(results, entry)
        end

        -- Skipped: "  ○ skipped test name" or "  ○ test name"
        local skip_name = line:match("[○∘]%s+skipped%s+(.+)%s*$")
        if not skip_name then
            skip_name = line:match("[○∘]%s+(.+)%s*$")
        end
        if skip_name then
            table.insert(results, {
                name = skip_name,
                suite = current_suite,
                state = "skipped",
                file = current_file,
            })
        end
    end

    return results
end
