import sys

def move_tests():
    with open('src/main.rs', 'r') as f:
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

    with open('src/main.rs', 'w') as f:
        f.writelines(main_lines)

    # Add `mod cli_tests;` to the end of main.rs (or at the top)
    # Actually, we can just write it to a file and include it at the top of main.rs
    with open('src/cli_tests.rs', 'w') as f:
        f.writelines(test_lines)

if __name__ == '__main__':
    move_tests()
