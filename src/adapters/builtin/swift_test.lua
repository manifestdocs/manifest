-- Adapter for `swift test` output (Swift/XCTest).
--
-- Parses the standard swift test output format:
--   Test Case '-[AuthTests testLogin]' started.
--   Test Case '-[AuthTests testLogin]' passed (0.001 seconds).
--   Test Case '-[AuthTests testInvalidPassword]' failed (0.002 seconds).
--
-- Swift 5.9+ format (swift-testing):
--   Test "login with valid credentials" started.
--   Test "login with valid credentials" passed after 0.001 seconds.

function parse(output)
    local results = {}
    local failure_messages = {}

    -- Collect failure messages
    -- Format: "/path/AuthTests.swift:42: error: -[AuthTests testInvalidPassword] : XCTAssertTrue failed"
    for line in output:gmatch("[^\r\n]+") do
        local file, test_line, class, method, msg =
            line:match("([^:]+):(%d+): error: %-?%[(%w+) (%w+)%]%s*:%s*(.+)")
        if class and method then
            local key = class .. "." .. method
            failure_messages[key] = {
                message = msg,
                file = file,
                line = tonumber(test_line),
            }
        end
    end

    -- Parse XCTest format result lines
    for line in output:gmatch("[^\r\n]+") do
        local class, method, status, dur

        -- Passed: "Test Case '-[AuthTests testLogin]' passed (0.001 seconds)."
        local p_class, p_method, p_dur = line:match(
            "Test Case '%-?%[(%w+) (%w+)%]' passed %(([%d%.]+) seconds%)"
        )
        if p_class then
            class = p_class
            method = p_method
            status = "passed"
            dur = p_dur
        end

        -- Failed: "Test Case '-[AuthTests testLogin]' failed (0.002 seconds)."
        if not class then
            local f_class, f_method, f_dur = line:match(
                "Test Case '%-?%[(%w+) (%w+)%]' failed %(([%d%.]+) seconds%)"
            )
            if f_class then
                class = f_class
                method = f_method
                status = "failed"
                dur = f_dur
            end
        end

        if class and method then
            local key = class .. "." .. method
            local failure = failure_messages[key]

            local entry = {
                name = method,
                suite = class,
                state = status,
            }

            if dur then
                entry.duration_ms = math.floor(tonumber(dur) * 1000)
            end

            if failure then
                entry.message = failure.message
                entry.file = failure.file
                entry.line = failure.line
            end

            table.insert(results, entry)
        end
    end

    -- Try swift-testing format (Swift 5.9+)
    -- "◇ Test testLogin started."
    -- "✔ Test testLogin passed after 0.001 seconds."
    -- "✘ Test testInvalidPassword failed after 0.002 seconds."
    if #results == 0 then
        for line in output:gmatch("[^\r\n]+") do
            -- Passed
            local p_name, p_dur = line:match("[✔◆]%s+Test (.+) passed after ([%d%.]+) seconds")
            if p_name then
                table.insert(results, {
                    name = p_name,
                    state = "passed",
                    duration_ms = math.floor(tonumber(p_dur) * 1000),
                })
            end

            -- Failed
            local f_name, f_dur = line:match("[✘]%s+Test (.+) failed after ([%d%.]+) seconds")
            if f_name then
                table.insert(results, {
                    name = f_name,
                    state = "failed",
                    duration_ms = math.floor(tonumber(f_dur) * 1000),
                })
            end

            -- Skipped
            local s_name = line:match("[↩⊘]%s+Test (.+) skipped")
            if s_name then
                table.insert(results, {
                    name = s_name,
                    state = "skipped",
                })
            end
        end
    end

    return results
end
