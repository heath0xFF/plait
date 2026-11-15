-- Use named accessibility controls only; global keystrokes can reach another application.
on assertThat(condition, message)
    if not condition then error message
end assertThat

on visibleText()
    tell application "System Events" to tell process "plait"
        set root to group "Plait" of group 1 of window "Plait"
        return ((name of every group of root) as text) & ((value of every static text of root) as text)
    end tell
end visibleText

on waitForText(expected)
    repeat 100 times
        try
            if my visibleText() contains expected then return
        end try
        delay 0.1
    end repeat
    error "Timed out waiting for: " & expected
end waitForText

on toggleReadTool()
    tell application "System Events" to tell process "plait"
        set root to group "Plait" of group 1 of window "Plait"
        set toolRow to first group of root whose name starts with "Read · project context"
        click button "Read · project context · mock tool event" of toolRow
    end tell
end toggleReadTool

on run arguments
    if (count arguments) is not 1 then error "Usage: smoke-macos.applescript dirty|clean|not-a-repository"
    set mode to item 1 of arguments
    tell application "System Events"
        repeat 100 times
            if exists group "Plait" of group 1 of window "Plait" of process "plait" then exit repeat
            delay 0.1
        end repeat
        tell process "plait"
            set root to group "Plait" of group 1 of window "Plait"
            click radio button "Changes" of root
        end tell
    end tell
    if mode is "clean" then
        my waitForText("Working tree clean")
        return "PASS: clean repository"
    else if mode is "not-a-repository" then
        my waitForText("Cannot read repository")
        tell application "System Events" to tell process "plait"
            my assertThat(exists button "Choose another repository…" of root, "Missing recovery control")
        end tell
        return "PASS: non-repository error and recovery control"
    end if
    my waitForText("Unified diff")
    tell application "System Events" to tell process "plait"
        click radio button "Conversation" of root
    end tell
    delay 0.3
    my assertThat(my visibleText() does not contain "README.md", "Tool details should start collapsed")
    my toggleReadTool()
    my waitForText("README.md")
    my toggleReadTool()
    delay 0.3
    my assertThat(my visibleText() does not contain "README.md", "Tool details did not collapse")
    my toggleReadTool()
    tell application "System Events" to tell process "plait"
        set tree to outline "Projects, machines, host threads and labeled fixtures" of root
        perform action "AXPress" of row "Review Git boundaries, Needs input (mock)" of tree
        perform action "AXPress" of row "Native desktop foundation, Ready (mock)" of tree
    end tell
    delay 0.3
    my assertThat(my visibleText() does not contain "README.md", "Tool details leaked across thread selection")
    tell application "System Events" to tell process "plait"
        click radio button "Changes" of root
        set tree to outline "Projects, machines, host threads and labeled fixtures" of root
        set originalCount to count rows of tree
        my assertThat(originalCount = 11, "Expected fresh fixture navigation")
        perform action "AXPress" of row 1 of tree
        delay 0.3
        my assertThat((count rows of tree) < originalCount, "Project did not collapse")
        perform action "AXPress" of row 1 of tree
        delay 0.3
        my assertThat((count rows of tree) = originalCount, "Project did not expand")
        perform action "AXPress" of row "Zaphod, local Git" of tree
        delay 0.3
        my assertThat((count rows of tree) = originalCount - 2, "Machine did not collapse")
        perform action "AXPress" of row "Zaphod, local Git" of tree
        delay 0.3
        perform action "AXPress" of row "Repository discovery, Working (mock)" of tree
    end tell
    my waitForText("Remote Git is not connected")
    tell application "System Events" to tell process "plait"
        perform action "AXPress" of row "A fresh conversation, Ready (mock)" of tree
        click radio button "Conversation" of root
    end tell
    my waitForText("A quiet place to start")
    tell application "System Events" to tell process "plait"
        perform action "AXPress" of row "Native desktop foundation, Ready (mock)" of tree
        click button "Open repository…" of root
        repeat 50 times
            if exists window "Open" then exit repeat
            delay 0.1
        end repeat
        my assertThat(exists window "Open", "Native folder picker did not open")
        click button "Cancel" of splitter group 1 of window "Open"
        delay 0.3
        click radio button "Changes" of root
        delay 0.3
        click button "review.rs, Modified, staged unstaged" of root
    end tell
    my waitForText("+    notify_when_ready();")
    tell application "System Events" to tell process "plait"
        click radio button "Staged" of root
    end tell
    my waitForText("+    queue_status();")
    my assertThat(my visibleText() does not contain "+    notify_when_ready();", "Staged view contains worktree-only changes")
    tell application "System Events" to tell process "plait"
        repeat 5 times
            click radio button "Unstaged" of root
            click radio button "Staged" of root
        end repeat
    end tell
    my waitForText("+    queue_status();")
    return "PASS: tool disclosure/reset, hierarchy, selection, remote/empty states, native picker/cancel, staged/unstaged diffs, rapid switching."
end run
