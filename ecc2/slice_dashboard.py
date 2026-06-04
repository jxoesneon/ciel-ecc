import sys
import re

def move_tests():
    with open('src/tui/dashboard.rs', 'r') as f:
        lines = f.readlines()

    test_start = -1
    for i, line in enumerate(lines):
        if line.startswith('#[cfg(test)]') and 'mod tests {' in lines[i+1]:
            test_start = i
            break

    if test_start == -1:
        print("Could not find tests")
        return

    main_lines = lines[:test_start]
    test_lines = lines[test_start:]

    with open('src/tui/dashboard.rs', 'w') as f:
        f.writelines(main_lines)

    with open('src/tui/dashboard_tests.rs', 'w') as f:
        f.writelines(test_lines)
        
    # fix super::* in tests
    with open('src/tui/dashboard_tests.rs', 'r') as f:
        tc = f.read()
    # Wait, the tests module uses `use super::*;`. If we move it to a sibling module `dashboard_tests`, 
    # it needs `use crate::tui::dashboard::*;` or we can keep it as `#[cfg(test)]\npub mod dashboard_tests;`
    # inside tui/mod.rs? 
    # Actually, in `tui/mod.rs` we can just do:
    # #[cfg(test)]
    # mod dashboard_tests;
    # Then `super::*` in dashboard_tests would point to `tui`, but it needs to point to `tui::dashboard`.
    # Let's replace `use super::*;` with `use super::dashboard::*;`
    tc = tc.replace('use super::*;', 'use super::dashboard::*;')
    with open('src/tui/dashboard_tests.rs', 'w') as f:
        f.write(tc)

if __name__ == '__main__':
    move_tests()
