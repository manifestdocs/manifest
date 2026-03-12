-- Adapter for JUnit output via Maven Surefire and Gradle.
--
-- Parses Maven Surefire output format:
--   [INFO] Running com.example.AuthTest
--   [INFO] Tests run: 3, Failures: 1, Errors: 0, Skipped: 1
--
-- Parses Gradle test output format:
--   > Task :test
--   com.example.AuthTest > loginWithValidCredentials PASSED
--   com.example.AuthTest > loginWithInvalidPassword FAILED
--
-- Also handles Maven failure details.

function parse(output)
    local results = {}
    local failure_messages = {}

    -- Collect failure details from Maven output
    -- Format:
    --   [ERROR] loginWithInvalidPassword(com.example.AuthTest)
    --   [ERROR]   expected: <true> but was: <false>
    local current_failure = nil
    local current_lines = {}

    for line in output:gmatch("[^\r\n]+") do
        local fail_method, fail_class = line:match("%[ERROR%]%s+(%w+)%(([%w%.]+)%)")
        if fail_method and fail_class then
            if current_failure then
                failure_messages[current_failure] = table.concat(current_lines, "\n")
            end
            current_failure = fail_class .. "." .. fail_method
            current_lines = {}
        elseif current_failure then
            local err_line = line:match("%[ERROR%]%s+(.+)")
            if err_line then
                table.insert(current_lines, err_line)
            else
                failure_messages[current_failure] = table.concat(current_lines, "\n")
                current_failure = nil
                current_lines = {}
            end
        end
    end
    if current_failure then
        failure_messages[current_failure] = table.concat(current_lines, "\n")
    end

    -- Try Gradle format first:
    -- "com.example.AuthTest > loginWithValidCredentials PASSED"
    -- "com.example.AuthTest > loginWithValidCredentials() PASSED"
    for line in output:gmatch("[^\r\n]+") do
        local class_name, method, status
        for _, s in ipairs({"PASSED", "FAILED", "SKIPPED"}) do
            local c, m = line:match("^([%w%.]+)%s+>%s+(.-)%s+" .. s .. "%s*$")
            if c and m then
                class_name = c
                method = m
                status = s
                break
            end
        end

        if class_name and method and status then
            local state
            if status == "PASSED" then
                state = "passed"
            elseif status == "FAILED" then
                state = "failed"
            else
                state = "skipped"
            end

            -- Strip trailing () from method name if present
            method = method:gsub("%(%s*%)%s*$", "")

            table.insert(results, {
                name = method,
                suite = class_name,
                state = state,
                message = failure_messages[class_name .. "." .. method],
            })
        end
    end

    -- If Gradle format found results, return them
    if #results > 0 then
        return results
    end

    -- Try Maven Surefire format
    -- Track current test class from "[INFO] Running com.example.AuthTest"
    local current_class = nil

    for line in output:gmatch("[^\r\n]+") do
        local class = line:match("%[INFO%] Running ([%w%.]+)")
        if class then
            current_class = class
        end

        -- Summary line: "[INFO] Tests run: 3, Failures: 1, Errors: 0, Skipped: 1"
        local run, failures, errors, skipped = line:match(
            "Tests run:%s*(%d+),%s*Failures:%s*(%d+),%s*Errors:%s*(%d+),%s*Skipped:%s*(%d+)"
        )
        if run and current_class then
            run = tonumber(run)
            failures = tonumber(failures)
            errors = tonumber(errors)
            skipped = tonumber(skipped)
            local passed = run - failures - errors - skipped

            -- Maven summary doesn't give individual test names
            -- Create aggregate entries per class
            if passed > 0 then
                table.insert(results, {
                    name = current_class .. " (" .. passed .. " passed)",
                    suite = current_class,
                    state = "passed",
                })
            end
            if failures > 0 then
                table.insert(results, {
                    name = current_class .. " (" .. failures .. " failed)",
                    suite = current_class,
                    state = "failed",
                })
            end
            if errors > 0 then
                table.insert(results, {
                    name = current_class .. " (" .. errors .. " errored)",
                    suite = current_class,
                    state = "errored",
                })
            end
            if skipped > 0 then
                table.insert(results, {
                    name = current_class .. " (" .. skipped .. " skipped)",
                    suite = current_class,
                    state = "skipped",
                })
            end
        end
    end

    return results
end
