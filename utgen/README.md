# utgen

This tool is used to generate unit tests, run them, and collect related data.

## Prerequisites

Create a file named `api.json` in the `res` folder with the following content:

```json
{
    "base": "https://xxxx/v1",
    "key": "sk-xxxxxxxxxx",
    "model": "xxx"
}
```

## Build

To build utgen, run `cargo build` in the `utgen` directory.

## Usage

`utgen --help`：

```
Generate unit tests for a project

Usage: utgen <COMMAND>

Commands:
  pre-process  Preprocess the project, including renaming integration tests and commenting out unit tests
  gen          Generate and run tests for the project to collect coverage data
  fix          Fix and run tests for the project to collect coverage data
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### Workflow

The main commands used are `pre-process`, `gen`, and `fix`. The workflow is to first run `utgen pre-process` for preprocessing, then run `utgen gen` to generate tests and collect data before fixes, and finally run `utgen fix` to fix the tests and collect data. The parameters for each command are explained below:

### pre-process

```sh
utgen pre-process --help
```

The output is as follows:

```
Preprocess the project, including renaming integration tests and commenting out unit tests

Usage: utgen pre-process [OPTIONS] --project-dir <PROJECT_DIR>

Options:
  -p, --project-dir <PROJECT_DIR>  Path to the project directory, can be relative to the current directory
  -w, --work-dir <WORK_DIR>        Path to the work directory(s), separated by commas (e.g., dir1,dir2), can be relative to the project directory, default to the project directory
  -h, --help                       Print help
```

The `pre-process` command will preprocess the selected project, including renaming existing integration test folders and commenting out existing unit tests. The parameters are as follows:

`-p`：Specify the path to the project for which tests need to be generated.

`-w`：If the project contains multiple subprojects, specify the paths to the subprojects.

### gen

```sh
utgen gen --help
```

The output is as follows:

```
Generate and run tests for the project to collect coverage data

Usage: utgen gen [OPTIONS] --project-dir <PROJECT_DIR>

Options:
  -p, --project-dir <PROJECT_DIR>  Path to the project directory, can be relative to the current directory
  -w, --work-dir <WORK_DIR>        Path to the work directory(s), separated by commas (e.g., dir1,dir2), can be relative to the project directory, default to the project directory
  -t, --tasks <TASKS>              Number of parallel test generation tasks [default: 128]
  -i, --integration                Whether to generate integration tests
  -r, --requirement                Whether to provide requirements in prompt
  -c, --context                    Whether to provide context in prompt
  -o, --oracle                     Whether to generate oracle independently
```

The `gen` command will generate tests for the selected project but will not fix them. It will then run the tests to collect data. The parameters are as follows:

`-p`：Specify the path to the project for which tests need to be generated.

`-w`：If the project contains multiple subprojects, specify the paths to the subprojects.

`-i`：Specify this option to run in integration test mode, meaning all generated tests will be placed in the `tests` folder, and only public functions or methods will be tested.

`-r`: Specify this option to ask LLM to generate tests for each condition chain.

`-c`: Specify this option to provide context of the focal function to the LLM.


### fix

```sh
utgen fix --help
```

The output is as follows:

```
Fix and run tests for the project to collect coverage data

Usage: utgen fix [OPTIONS] --project-dir <PROJECT_DIR>

Options:
  -p, --project-dir <PROJECT_DIR>  Path to the project directory, can be relative to the current directory
  -w, --work-dir <WORK_DIR>        Path to the work directory(s), separated by commas (e.g., dir1,dir2), can be relative to the project directory, default to the project directory
  -h, --help                       Print help
```

The `fix` command will attempt to fix the generated tests for the selected project and then run the tests to collect data. The parameters are as follows:

`-p`：Specify the path to the project for which tests need to be generated.

`-w`：If the project contains multiple subprojects, specify the paths to the subprojects.
