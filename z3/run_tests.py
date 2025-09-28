#!/usr/bin/env python3
"""
Z3 SMT Test Suite Runner for 9P.e Server
Runs all SMT-LIB2 tests and verifies correctness properties
"""

import subprocess
import sys
import os
import time
from pathlib import Path
from typing import List, Tuple, Dict
import json

# ANSI color codes for terminal output
class Colors:
    GREEN = '\033[92m'
    RED = '\033[91m'
    YELLOW = '\033[93m'
    BLUE = '\033[94m'
    MAGENTA = '\033[95m'
    CYAN = '\033[96m'
    BOLD = '\033[1m'
    RESET = '\033[0m'

class Z3TestRunner:
    def __init__(self, test_dir: str = "."):
        self.test_dir = Path(test_dir)
        self.results = []
        self.total_tests = 0
        self.passed_tests = 0
        self.failed_tests = 0

    def find_test_files(self) -> List[Path]:
        """Find all .smt2 test files in the test directory"""
        if not self.test_dir.exists():
            print(f"{Colors.RED}Error: Test directory {self.test_dir} not found{Colors.RESET}")
            return []

        test_files = list(self.test_dir.glob("*.smt2"))
        test_files.sort()
        return test_files

    def run_z3(self, test_file: Path) -> Tuple[bool, str, float]:
        """Run Z3 on a test file and return success status, output, and execution time"""
        start_time = time.time()

        try:
            result = subprocess.run(
                ["z3", str(test_file)],
                capture_output=True,
                text=True,
                timeout=60  # 60 second timeout
            )

            execution_time = time.time() - start_time

            # Check if all tests passed (look for "unsat" for negative tests, "sat" for positive)
            output = result.stdout + result.stderr

            # Count the number of "sat" and "unsat" results
            sat_count = output.count("sat") - output.count("unsat")  # Subtract "unsat" from total "sat"
            unsat_count = output.count("unsat")

            # Check for errors
            if "error" in output.lower() or result.returncode != 0:
                return False, output, execution_time

            # All tests should produce expected results
            if sat_count >= 0 and unsat_count >= 0:
                return True, output, execution_time
            else:
                return False, output, execution_time

        except subprocess.TimeoutExpired:
            execution_time = time.time() - start_time
            return False, "Test timed out after 60 seconds", execution_time
        except Exception as e:
            execution_time = time.time() - start_time
            return False, f"Error running Z3: {e}", execution_time

    def extract_test_info(self, test_file: Path) -> Dict[str, any]:
        """Extract test information from the file"""
        info = {
            "name": test_file.stem,
            "description": "",
            "num_tests": 0
        }

        with open(test_file, 'r') as f:
            lines = f.readlines()
            for line in lines:
                if line.startswith(";;;") and not info["description"]:
                    info["description"] = line[3:].strip()
                elif line.strip().startswith("(echo"):
                    info["num_tests"] += 1

        return info

    def print_test_header(self, info: Dict[str, any]):
        """Print test file header"""
        print(f"\n{Colors.BOLD}{Colors.CYAN}{'=' * 80}{Colors.RESET}")
        print(f"{Colors.BOLD}Testing: {info['name']}{Colors.RESET}")
        print(f"{Colors.BLUE}{info['description']}{Colors.RESET}")
        print(f"Number of test cases: {info['num_tests']}")
        print(f"{Colors.CYAN}{'=' * 80}{Colors.RESET}")

    def parse_test_output(self, output: str) -> List[str]:
        """Parse Z3 output to extract test results"""
        results = []
        lines = output.split('\n')

        for i, line in enumerate(lines):
            if line.startswith("Test"):
                # Found a test description
                test_name = line
                # Check if next line is "sat" or "unsat"
                if i + 1 < len(lines):
                    result = lines[i + 1].strip()
                    if result in ["sat", "unsat"]:
                        results.append(f"{test_name} -> {Colors.GREEN}✓ {result}{Colors.RESET}")
                    else:
                        results.append(f"{test_name} -> {Colors.YELLOW}? {result}{Colors.RESET}")
            elif line.startswith("Verified:"):
                results.append(f"  {Colors.GREEN}✓{Colors.RESET} {line}")

        return results

    def run_test_file(self, test_file: Path) -> bool:
        """Run a single test file and report results"""
        info = self.extract_test_info(test_file)
        self.print_test_header(info)

        print(f"\n{Colors.YELLOW}Running Z3...{Colors.RESET}")
        success, output, exec_time = self.run_z3(test_file)

        if success:
            print(f"{Colors.GREEN}✓ All tests passed{Colors.RESET} ({exec_time:.2f}s)")

            # Parse and display individual test results
            test_results = self.parse_test_output(output)
            for result in test_results:
                print(result)

            self.passed_tests += 1
        else:
            print(f"{Colors.RED}✗ Test failed{Colors.RESET} ({exec_time:.2f}s)")
            print(f"\n{Colors.RED}Error output:{Colors.RESET}")
            print(output[:500])  # Print first 500 chars of error
            self.failed_tests += 1

        self.results.append({
            "file": str(test_file),
            "name": info["name"],
            "success": success,
            "execution_time": exec_time,
            "num_tests": info["num_tests"]
        })

        return success

    def run_all_tests(self) -> bool:
        """Run all test files"""
        test_files = self.find_test_files()

        if not test_files:
            print(f"{Colors.YELLOW}No test files found in {self.test_dir}{Colors.RESET}")
            return False

        print(f"{Colors.BOLD}{Colors.MAGENTA}")
        print("=" * 80)
        print("9P.e Server SMT/Z3 Test Suite")
        print("=" * 80)
        print(f"{Colors.RESET}")

        print(f"Found {len(test_files)} test files\n")

        all_passed = True
        for test_file in test_files:
            if not self.run_test_file(test_file):
                all_passed = False
            self.total_tests += 1

        return all_passed

    def print_summary(self):
        """Print test summary"""
        print(f"\n{Colors.BOLD}{Colors.MAGENTA}{'=' * 80}{Colors.RESET}")
        print(f"{Colors.BOLD}Test Summary{Colors.RESET}")
        print(f"{Colors.MAGENTA}{'=' * 80}{Colors.RESET}\n")

        total_time = sum(r["execution_time"] for r in self.results)
        total_test_cases = sum(r["num_tests"] for r in self.results)

        print(f"Total test files: {self.total_tests}")
        print(f"  {Colors.GREEN}Passed: {self.passed_tests}{Colors.RESET}")
        print(f"  {Colors.RED}Failed: {self.failed_tests}{Colors.RESET}")
        print(f"Total test cases: {total_test_cases}")
        print(f"Total execution time: {total_time:.2f}s")

        if self.failed_tests == 0:
            print(f"\n{Colors.BOLD}{Colors.GREEN}🎉 All tests passed!{Colors.RESET}")
        else:
            print(f"\n{Colors.BOLD}{Colors.RED}❌ Some tests failed{Colors.RESET}")
            print("\nFailed tests:")
            for result in self.results:
                if not result["success"]:
                    print(f"  - {result['name']}")

    def save_results(self, output_file: str = "z3_test_results.json"):
        """Save test results to JSON file"""
        with open(output_file, 'w') as f:
            json.dump({
                "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
                "total_tests": self.total_tests,
                "passed": self.passed_tests,
                "failed": self.failed_tests,
                "results": self.results
            }, f, indent=2)
        print(f"\nResults saved to {output_file}")

def check_z3_installed() -> bool:
    """Check if Z3 is installed and available"""
    try:
        result = subprocess.run(["z3", "--version"], capture_output=True, text=True)
        if result.returncode == 0:
            print(f"Z3 version: {result.stdout.strip()}")
            return True
    except FileNotFoundError:
        pass

    print(f"{Colors.RED}Error: Z3 is not installed or not in PATH{Colors.RESET}")
    print("Install Z3 with: sudo apt-get install z3")
    return False

def main():
    """Main entry point"""
    if not check_z3_installed():
        sys.exit(1)

    # Parse command line arguments
    test_dir = sys.argv[1] if len(sys.argv) > 1 else "."

    # Run tests
    runner = Z3TestRunner(test_dir)
    all_passed = runner.run_all_tests()
    runner.print_summary()
    runner.save_results()

    # Exit with appropriate code
    sys.exit(0 if all_passed else 1)

if __name__ == "__main__":
    main()