import argparse


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--heat", "-H", type=int)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()
