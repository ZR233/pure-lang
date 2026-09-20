"""Run main-agent acceptance using the shared isolated Linux Driver lifecycle."""
import sys

from sidebar_native_harness import run


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('usage: python3 code/anywork/tool/main_agent_native_harness.py OUTPUT_DIRECTORY')
    run(sys.argv[1], driver='main_agent_acceptance_driver.dart')
