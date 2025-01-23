import 'zx/globals';
import { JsonMap, parse as parseToml } from '@iarna/toml';

process.env.FORCE_COLOR = '3';
process.env.CARGO_TERM_COLOR = 'always';

export const workingDirectory = (await $`pwd`.quiet()).toString().trim();

export function getAllProgramIdls(): string[] {
  return getAllProgramFolders().map((folder) =>
    path.join(workingDirectory, folder, 'idl.json')
  );
}

export function getExternalProgramOutputDir(): string {
  const config = getCargoMetadata()?.solana?.['external-programs-output'];
  return path.join(workingDirectory, config ?? 'target/deploy');
}

export function getExternalProgramAddresses(): string[] {
  const addresses = getProgramFolders().flatMap(
    (folder) => getCargoMetadata(folder)?.solana?.['program-dependencies'] ?? []
  );
  return Array.from(new Set(addresses));
}

export function getExternalAccountAddresses(): string[] {
  const addresses = getProgramFolders().flatMap(
    (folder) => getCargoMetadata(folder)?.solana?.['account-dependencies'] ?? []
  );
  return Array.from(new Set(addresses));
}

let didWarnAboutMissingPrograms = false;
export function getProgramFolders(): string[] {
  let programs;

  if (process.env.PROGRAMS) {
    try {
      programs = JSON.parse(process.env.PROGRAMS);
    } catch (error) {
      programs = process.env.PROGRAMS.split(/\s+/);
    }
  } else {
    programs = getAllProgramFolders();
  }

  const filteredPrograms = programs.filter((program) =>
    fs.existsSync(path.join(workingDirectory, program))
  );

  if (
    filteredPrograms.length !== programs.length &&
    !didWarnAboutMissingPrograms
  ) {
    didWarnAboutMissingPrograms = true;
    programs
      .filter((program) => !filteredPrograms.includes(program))
      .forEach((program) => {
        echo(chalk.yellow(`Program not found: ${workingDirectory}/${program}`));
      });
  }

  return filteredPrograms;
}

export function getAllProgramFolders(): string[] {
  return getCargo().workspace['members'].filter(
    (member) => getCargo(member).package['metadata']?.['solana']?.['program-id']
  );
}

export function getCargo(folder?: string): JsonMap {
  return parseToml(
    fs.readFileSync(
      path.join(workingDirectory, folder ? folder : '.', 'Cargo.toml'),
      'utf8'
    )
  );
}

export function getCargoMetadata(folder?: string) {
  const cargo = getCargo(folder);
  return folder ? cargo?.package?.['metadata'] : cargo?.workspace?.['metadata'];
}

export function getSolanaVersion(): string {
  return getCargoMetadata()?.cli?.solana;
}

export function getToolchain(operation): string {
  return getCargoMetadata()?.toolchains?.[operation];
}

export function getToolchainArgument(operation): string {
  const channel = getToolchain(operation);
  return channel ? `+${channel}` : '';
}

export function cliArguments(): string[] {
  return process.argv.slice(2);
}

export function popArgument(args: string[], arg: string) {
  const index = args.indexOf(arg);
  if (index >= 0) {
    args.splice(index, 1);
  }
  return index >= 0;
}

export function partitionArguments(
  args: string[],
  delimiter: string,
  defaultArgs?: string[]
): [string[], string[]] {
  const index = args.indexOf(delimiter);
  const [providedCargoArgs, providedCommandArgs] =
    index >= 0 ? [args.slice(0, index), args.slice(index + 1)] : [args, []];

  if (defaultArgs) {
    const [defaultCargoArgs, defaultCommandArgs] = partitionArguments(
      defaultArgs,
      delimiter
    );
    return [
      [...defaultCargoArgs, ...providedCargoArgs],
      [...defaultCommandArgs, ...providedCommandArgs],
    ];
  }
  return [providedCargoArgs, providedCommandArgs];
}

export async function getInstalledSolanaVersion(): Promise<string | undefined> {
  try {
    const { stdout } = await $`solana --version`.quiet();
    return stdout.match(/(\d+\.\d+\.\d+)/)?.[1];
  } catch (error) {
    return '';
  }
}

export function parseCliArguments(): {
  command: string;
  libraryPath: string;
  args: string[];
} {
  const command = process.argv[2];
  const args = process.argv.slice(3);

  // Extract the relative crate directory from the command-line arguments. This
  // is the only required argument.
  const relativePath = args.shift();

  if (!relativePath) {
    throw new Error('Missing relative manifest path');
  }

  return {
    command,
    libraryPath: path.join(workingDirectory, relativePath),
    args,
  };
}
