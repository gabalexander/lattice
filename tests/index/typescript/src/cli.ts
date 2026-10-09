import { Command } from "commander";

const program = new Command();
program.command("weave <pattern>").option("-w, --width <n>", "the width");
program.parse();
