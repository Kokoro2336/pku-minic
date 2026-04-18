#!/usr/bin/env python3
import argparse
import os
import subprocess
import sys
import shutil
import shlex
import uuid

def run_command(command, capture_output=True):
    """Runs a shell command."""
    result = subprocess.run(
        command,
        shell=True,
        stdout=subprocess.PIPE if capture_output else None,
        stderr=subprocess.PIPE if capture_output else None
    )
    return result

def find_files(directory, extension):
    """Recursively finds files with a specific extension."""
    matches = []
    for root, dirnames, filenames in os.walk(directory):
        for filename in filenames:
            if filename.endswith(extension):
                matches.append(os.path.join(root, filename))
    return matches

def basic_has_selector(raw_args):
    """Returns True if --basic was provided with an explicit value."""
    for i, arg in enumerate(raw_args):
        if arg == "--basic":
            return i + 1 < len(raw_args) and not raw_args[i + 1].startswith("-")
        if arg.startswith("--basic="):
            return True
    return False

def matches_test_id(test_path, test_id, h_functional_dir):
    """Matches test IDs using the same conventions as --test/--basic selectors."""
    basename = os.path.basename(test_path)
    abs_test_path = os.path.abspath(test_path)
    abs_hidden_dir = os.path.abspath(h_functional_dir)
    is_hidden_test = os.path.commonpath([abs_test_path, abs_hidden_dir]) == abs_hidden_dir

    if test_id.startswith("h"):
        search_prefix = test_id[1:]
        if not search_prefix or not is_hidden_test:
            return False
    else:
        search_prefix = test_id
        if not search_prefix or is_hidden_test:
            return False

    target_name = search_prefix + ".sy"
    return basename == target_name or basename.startswith(search_prefix + "_")

def collect_perf_tests(perf_dir, perf_selectors):
    """Collects perf tests; selectors must be exact benchmark names without .sy."""
    all_perf_sy = find_files(perf_dir, ".sy")
    all_perf_sy.sort()

    if len(perf_selectors) == 0 or "all" in perf_selectors:
        return all_perf_sy, []

    benchmark_to_files = {}
    for test_file in all_perf_sy:
        benchmark = os.path.splitext(os.path.basename(test_file))[0]
        benchmark_to_files.setdefault(benchmark, []).append(test_file)

    missing = []
    selected = []
    for benchmark in perf_selectors:
        matched = benchmark_to_files.get(benchmark)
        if matched:
            selected.extend(matched)
        else:
            missing.append(benchmark)

    if missing:
        return None, missing

    # Remove duplicates while preserving selector order.
    deduped = []
    seen = set()
    for test_file in selected:
        if test_file not in seen:
            deduped.append(test_file)
            seen.add(test_file)
    return deduped, []

def clean_directory(directory):
    """Removes all files in a directory."""
    if os.path.exists(directory):
        for filename in os.listdir(directory):
            file_path = os.path.join(directory, filename)
            try:
                if os.path.isfile(file_path) or os.path.islink(file_path):
                    os.unlink(file_path)
                elif os.path.isdir(file_path):
                    shutil.rmtree(file_path)
            except Exception as e:
                print(f'Failed to delete {file_path}. Reason: {e}')
    else:
        os.makedirs(directory)

def normalize_output_bytes(data: bytes) -> bytes:
    normalized = data.replace(b"\r\n", b"\n")
    lines = normalized.split(b"\n")
    while lines and lines[0].strip() == b"":
        lines.pop(0)
    while lines and lines[-1].strip() == b"":
        lines.pop()
    return b"\n".join(lines)

def generate_cfg_graphs(ll_path: str, graph_dir: str, test_name: str):
    os.makedirs(graph_dir, exist_ok=True)
    abs_ll_path = os.path.abspath(ll_path)

    opt_commands = [
        ["opt", "-passes=dot-cfg", "-disable-output", "-disable-verify", abs_ll_path],
        ["opt", "-enable-new-pm=0", "-dot-cfg", "-disable-output", "-disable-verify", abs_ll_path],
        ["opt", "-enable-new-pm=0", "-dot-cfg-only", "-disable-output", "-disable-verify", abs_ll_path],
    ]

    opt_ok = False
    opt_stderr = b""
    for cmd in opt_commands:
        result = subprocess.run(cmd, cwd=graph_dir, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        opt_stderr += result.stderr
        if result.returncode == 0:
            opt_ok = True
            break

    if not opt_ok:
        return 1, b"", opt_stderr + b"\n[ERROR] Failed to generate CFG .dot via opt\n"

    dot_files = [f for f in os.listdir(graph_dir) if f.endswith(".dot")]
    if not dot_files:
        return 1, b"", b"[ERROR] opt succeeded but produced no .dot files\n"
    graphviz_stdout = b""
    graphviz_stderr = b""

    for dot_file in dot_files:
        dot_path = os.path.join(graph_dir, dot_file)
        svg_base = os.path.splitext(dot_file)[0]
        svg_path = os.path.join(graph_dir, f"{test_name}_{svg_base}.svg")
        dot_result = subprocess.run(
            ["dot", "-Tsvg", dot_path, "-o", svg_path],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        graphviz_stdout += dot_result.stdout
        graphviz_stderr += dot_result.stderr
        if dot_result.returncode != 0:
            return dot_result.returncode, graphviz_stdout, graphviz_stderr + b"\n[ERROR] Graphviz dot failed\n"

    return 0, graphviz_stdout, graphviz_stderr

def docker_image_exists(image_name: str) -> bool:
    result = subprocess.run(
        ["docker", "image", "inspect", image_name],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return result.returncode == 0

def start_qemu_container(image_name: str, container_name: str, repo_root: str):
    return subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "-d",
            "--name",
            container_name,
            "-v",
            f"{repo_root}:/workspace",
            "-w",
            "/workspace",
            image_name,
            "sleep",
            "infinity",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

def stop_qemu_container(container_name: str):
    subprocess.run(
        ["docker", "rm", "-f", container_name],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

def to_container_path(host_path: str, repo_root: str) -> str:
    rel = os.path.relpath(os.path.abspath(host_path), repo_root)
    return "/workspace/" + rel.replace(os.sep, "/")

def run_qemu_runtime(
    container_name: str,
    repo_root: str,
    asm_path: str,
    elf_path: str,
    input_file: str,
):
    asm_in_container = to_container_path(asm_path, repo_root)
    elf_in_container = to_container_path(elf_path, repo_root)
    sylib_in_container = "/workspace/sylib/sylib.c"

    gcc_cmd = (
        "riscv-gcc -O2 -static "
        f"-x assembler {shlex.quote(asm_in_container)} "
        f"-x c {shlex.quote(sylib_in_container)} "
        f"-o {shlex.quote(elf_in_container)}"
    )
    gcc_result = subprocess.run(
        ["docker", "exec", container_name, "bash", "-lc", gcc_cmd],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    if gcc_result.returncode != 0:
        return gcc_result.returncode, gcc_result.stdout, gcc_result.stderr, None, None

    run_cmd = f"qemu-riscv -L /usr/riscv64-linux-gnu {shlex.quote(elf_in_container)}"
    exec_cmd = ["docker", "exec", "-i", container_name, "bash", "-lc", run_cmd]
    if os.path.exists(input_file):
        with open(input_file, "rb") as f_in:
            run_result = subprocess.run(
                exec_cmd,
                stdin=f_in,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
    else:
        run_result = subprocess.run(
            exec_cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    runtime_raw = run_result.stdout
    runtime_with_ret = runtime_raw
    if not runtime_with_ret.endswith(b"\n"):
        runtime_with_ret += b"\n"
    runtime_with_ret += f"{run_result.returncode}\n".encode()

    merged_stdout = gcc_result.stdout + runtime_with_ret
    merged_stderr = gcc_result.stderr + run_result.stderr
    return run_result.returncode, merged_stdout, merged_stderr, runtime_with_ret, runtime_raw

def run_qemu_gdb_debug(
    container_name: str,
    repo_root: str,
    asm_path: str,
    elf_path: str,
    gdb_port: int = 1234,
):
    asm_in_container = to_container_path(asm_path, repo_root)
    elf_in_container = to_container_path(elf_path, repo_root)
    sylib_in_container = "/workspace/sylib/sylib.c"

    gcc_cmd = (
        "riscv-gcc -O2 -g -static "
        f"-x assembler {shlex.quote(asm_in_container)} "
        f"-x c {shlex.quote(sylib_in_container)} "
        f"-o {shlex.quote(elf_in_container)}"
    )
    gcc_result = subprocess.run(["docker", "exec", container_name, "bash", "-lc", gcc_cmd])
    if gcc_result.returncode != 0:
        return gcc_result.returncode

    qemu_cmd = (
        f"qemu-riscv -g {gdb_port} -L /usr/riscv64-linux-gnu "
        f"{shlex.quote(elf_in_container)}"
    )
    qemu_proc = subprocess.Popen(["docker", "exec", container_name, "bash", "-lc", qemu_cmd])

    # Prefer target-specific gdb, then multiarch, then generic gdb.
    gdb_detect = subprocess.run(
        [
            "docker",
            "exec",
            container_name,
            "bash",
            "-lc",
            "command -v riscv64-linux-gnu-gdb || command -v gdb-multiarch || command -v gdb",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    gdb_bin = gdb_detect.stdout.strip()
    if gdb_detect.returncode != 0 or not gdb_bin:
        print(
            "[ERROR] No gdb binary found in qemu container. "
            "Install riscv64-linux-gnu-gdb or gdb-multiarch, then rebuild image iroha-qemu-test."
        )
        return 127

    gdb_cmd = (
        f"{shlex.quote(gdb_bin)} -tui "
        f"{shlex.quote(elf_in_container)} "
        f"-ex 'target remote :{gdb_port}'"
    )

    try:
        gdb_result = subprocess.run(
            ["docker", "exec", "-it", container_name, "bash", "-lc", gdb_cmd]
        )
        return gdb_result.returncode
    finally:
        if qemu_proc.poll() is None:
            qemu_proc.terminate()
            try:
                qemu_proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                qemu_proc.kill()
                qemu_proc.wait(timeout=2)

def resolve_asm_artifact(work_dir: str, target_dir: str, test_name: str):
    candidates = [
        os.path.join(target_dir, f"{test_name}.asm"),
        os.path.join(target_dir, f"{test_name}.s"),
        os.path.join(work_dir, f"{test_name}.asm"),
        os.path.join(work_dir, f"{test_name}.s"),
    ]
    for path in candidates:
        if os.path.exists(path):
            return path

    for folder in [target_dir, work_dir]:
        if not os.path.isdir(folder):
            continue
        for filename in sorted(os.listdir(folder)):
            if filename.endswith(".asm") or filename.endswith(".s"):
                return os.path.join(folder, filename)

    return None

def finalize_qemu_cases(qemu_cases, test_output_base: str):
    moved_passed = 0
    moved_failed = 0

    for case in qemu_cases:
        if case.get("finalized", False):
            continue

        host_passed = case.get("host_passed", case.get("returncode", 1) == 0)
        runtime_done = case.get("runtime_done", False)
        finished = (not host_passed) or runtime_done
        if not finished:
            continue

        name_no_ext = case["name"]
        work_test_output_dir = case["work_dir"]
        status = "passed" if case["returncode"] == 0 else "failed"
        test_output_dir = os.path.join(test_output_base, status, name_no_ext)
        other_status = "failed" if status == "passed" else "passed"
        other_output_dir = os.path.join(test_output_base, other_status, name_no_ext)

        if os.path.exists(other_output_dir):
            shutil.rmtree(other_output_dir)
        if os.path.exists(test_output_dir):
            shutil.rmtree(test_output_dir)
        os.makedirs(os.path.dirname(test_output_dir), exist_ok=True)

        if os.path.exists(work_test_output_dir):
            shutil.move(work_test_output_dir, test_output_dir)

        case["finalized"] = True

        if case["returncode"] != 0:
            print(f"  [FAILED] {name_no_ext} (Exit Code: {case['returncode']})")
            moved_failed += 1
        else:
            print(f"  [PASSED] {name_no_ext}")
            moved_passed += 1

    return moved_passed, moved_failed

def main():
    parser = argparse.ArgumentParser(description='Compiler Test Runner')
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--test', type=str, help='Test file name (excluding .sy suffix) or test number')
    group.add_argument(
        '--basic',
        nargs='*',
        default=None,
        help='Test basic suites: no value means all; values can be multiple test ids (same style as --exclude), e.g. --basic 00 h29 82_long_func',
    )
    group.add_argument(
        '--perf',
        nargs='*',
        default=None,
        help='Test perf suites: no value means all; values must be full benchmark names without .sy, e.g. --perf 2025-UHB-58 2025-OKA-1',
    )
    group.add_argument(
        '--test-all',
        action='store_true',
        help='Run all basic and perf tests',
    )
    parser.add_argument(
        '--exclude',
        nargs='+',
        metavar='TEST_ID',
        help='Exclude test IDs when running --basic with no selector, e.g. --basic --exclude 00 h00 82_long_func',
    )
    parser.add_argument('--clean', action='store_true', help='Clean test directories before running')
    parser.add_argument('--graph', action='store_true', help='Generate CFG graphs (.dot/.svg) from linked LLVM IR using opt + graphviz')
    parser.add_argument('--trace', action='store_true', help='Enable trace logging')
    parser.add_argument('--no-debug', action='store_true', help='Disable cargo debug feature (enabled by default)')
    debug_group = parser.add_mutually_exclusive_group()
    debug_group.add_argument('--gdb', action='store_true', help='Run compiler under rust-gdb for interactive debugging (single test only)')
    debug_group.add_argument('--lldb', action='store_true', help='Run compiler under rust-lldb for interactive debugging (single test only)')
    parser.add_argument('--dump-llvm-after', type=str, default='', help='Dump LLVM IR after a specific pass (pass name)')
    parser.add_argument('--dump-asm-after', type=str, default='', help='Dump assembly after a specific backend pass (pass name)')
    parser.add_argument('--emit-llvm', action='store_true', help='Enable compiler --emit-llvm explicitly for dumping LLVM IR')
    backend_group = parser.add_mutually_exclusive_group()
    backend_group.add_argument('--lli', action='store_true', help='Use lli to interpret linked .ll')
    backend_group.add_argument('--llc', action='store_true', help='Use llc to compile linked .ll into executable and run it')
    backend_group.add_argument('--qemu', action='store_true', help='Compile on host, then run asm via riscv-gcc + qemu-riscv in docker (single container)')
    backend_group.add_argument('--qemu-debug', '--qemu-gdb', dest='qemu_debug', action='store_true', help='Compile on host, then launch qemu gdb-stub and open interactive riscv gdb (-tui) in docker')
    args = parser.parse_args()

    if args.exclude:
        if args.basic is None or len(args.basic) != 0:
            parser.error("--exclude is only allowed with --basic and no selector, e.g. --basic --exclude 00 h00")
        invalid_ids = [test_id for test_id in args.exclude if test_id == 'h']
        if invalid_ids:
            parser.error("Invalid --exclude test id: h")

    if args.basic is not None:
        invalid_ids = [test_id for test_id in args.basic if test_id == 'h']
        if invalid_ids:
            parser.error("Invalid --basic test id: h")

    if args.lli:
        exec_mode = 'lli'
    elif args.llc:
        exec_mode = 'llc'
    elif args.qemu:
        exec_mode = 'qemu'
    elif args.qemu_debug:
        exec_mode = 'qemu_debug'
    else:
        exec_mode = 'compiler'

    # LLVM dump is required for IR-level workflows and is auto-enabled for --lli.
    need_emit_llvm = args.emit_llvm or args.lli or args.llc or args.graph or bool(args.dump_llvm_after)
    need_runtime_exec = args.lli or args.llc

    if args.clean and not (args.test or args.basic is not None or args.perf is not None or args.test_all):
        clean_directory("./test")
        print("Cleaned test directory.")
        sys.exit(0)

    # Ensure cargo build is run
    print("Running cargo build...")
    build_cmd = "RUSTFLAG='-A warnings' cargo build --features debug"
    if args.no_debug:
        build_cmd = "RUSTFLAG='-A warnings' cargo build"
    build_result = run_command(build_cmd, capture_output=False)

    if build_result.returncode != 0:
        print("Build failed. Exiting.")
        sys.exit(1)

    compiler_binary = "./target/debug/compiler"
    if not os.path.exists(compiler_binary):
        print(f"Compiler binary not found at {compiler_binary}")
        sys.exit(1)

    repo_root = os.path.abspath(os.path.dirname(__file__))
    qemu_image = "iroha-qemu-test"
    qemu_container_name = f"iroha-qemu-{uuid.uuid4().hex[:8]}"
    qemu_cases = []

    if args.qemu or args.qemu_debug:
        if shutil.which("docker") is None:
            print("--qemu/--qemu-debug requested but docker was not found in PATH.")
            sys.exit(1)

        dockerfile_path = os.path.join(repo_root, "Dockerfile")
        if not os.path.exists(dockerfile_path):
            print(f"--qemu requested but Dockerfile was not found at {dockerfile_path}")
            sys.exit(1)

        print(f"Checking docker image: {qemu_image}")
        if not docker_image_exists(qemu_image):
            print(f"Docker image {qemu_image} not found. Building...")
            build_image_result = subprocess.run(
                ["docker", "build", "-t", qemu_image, "-f", dockerfile_path, repo_root],
                stdout=None,
                stderr=None,
            )
            if build_image_result.returncode != 0:
                print("Docker build failed. Exiting.")
                sys.exit(1)

    test_files = []
    basic_base_dir = "./testcases/functional_recover"
    functional_dir = os.path.join(basic_base_dir, "functional")
    h_functional_dir = os.path.join(basic_base_dir, "h_functional")
    perf_dir = "./testcases/perf"

    basic_search_dirs = [functional_dir, h_functional_dir]

    if args.test:
        # Find specific test file
        found = False
        if args.test.startswith('h'):
            search_prefix = args.test[1:]
            current_search_dirs = [h_functional_dir]
        else:
            search_prefix = args.test
            current_search_dirs = [functional_dir, h_functional_dir]

        target_name = search_prefix + ".sy"
        all_sy = []
        for d in current_search_dirs:
            all_sy.extend(find_files(d, ".sy"))
        for f in all_sy:
            basename = os.path.basename(f)
            if basename == target_name or basename.startswith(search_prefix + "_"):
                test_files.append(f)
                found = True
                break
        if not found:
            print(f"Test file {args.test} not found.")
            sys.exit(1)
    elif args.basic is not None:
        basic_selectors = args.basic
        all_sy = []
        for d in basic_search_dirs:
            all_sy.extend(find_files(d, ".sy"))

        if len(basic_selectors) == 0 or "all" in basic_selectors:
            test_files = all_sy
        else:
            test_files = [
                test_file for test_file in all_sy
                if any(matches_test_id(test_file, test_id, h_functional_dir) for test_id in basic_selectors)
            ]

            if not test_files:
                print(f"No test files matched basic selectors: {' '.join(basic_selectors)}")
                sys.exit(1)

        # Sort for consistent order
        test_files.sort()

        if args.exclude:
            selected_before = test_files
            test_files = [
                test_file for test_file in selected_before
                if not any(matches_test_id(test_file, test_id, h_functional_dir) for test_id in args.exclude)
            ]

            if len(test_files) == len(selected_before):
                print(f"[WARN] --exclude matched no tests: {' '.join(args.exclude)}")

            if not test_files:
                print("No test files remain after applying --exclude.")
                sys.exit(1)
    elif args.perf is not None:
        test_files, missing_perf = collect_perf_tests(perf_dir, args.perf)
        if test_files is None:
            print(f"No perf benchmark matched full name(s): {' '.join(missing_perf)}")
            sys.exit(1)
        if not test_files:
            print("No perf test files found.")
            sys.exit(1)
    elif args.test_all:
        all_basic_sy = []
        for d in basic_search_dirs:
            all_basic_sy.extend(find_files(d, ".sy"))
        all_basic_sy.sort()

        all_perf_sy, missing_perf = collect_perf_tests(perf_dir, [])
        if all_perf_sy is None:
            print(f"No perf benchmark matched full name(s): {' '.join(missing_perf)}")
            sys.exit(1)

        test_files = all_basic_sy + all_perf_sy

        if not test_files:
            print("No test files found for --test-all.")
            sys.exit(1)
    else:
        parser.print_help()
        sys.exit(1)

    debugger_tool = None
    debugger_flag = None
    if args.gdb:
        debugger_tool = "rust-gdb"
        debugger_flag = "--gdb"
    elif args.lldb:
        debugger_tool = "rust-lldb"
        debugger_flag = "--lldb"

    if debugger_tool is not None:
        if args.qemu:
            parser.error("--qemu does not support debugger mode")
        if args.qemu_debug:
            parser.error("--qemu-debug already provides an interactive debugger")
        if shutil.which(debugger_tool) is None:
            print(f"{debugger_flag} requested but {debugger_tool} was not found in PATH.")
            sys.exit(1)
        if len(test_files) != 1:
            print(f"{debugger_flag} supports exactly one test. Please use --test <name>.")
            sys.exit(1)

    if args.qemu_debug:
        if len(test_files) != 1:
            parser.error("--qemu-debug supports exactly one test. Please use --test <name>.")

    # Directories to manage
    logs_dir = "./logs"
    graphs_dir = "./graphs"
    dump_llvm_dir = "./dump_llvm"
    dump_asm_dir = "./dump_asm"
    test_output_base = "./test"
    sylib_ll = "./sylib/sylib.ll"

    # clean test/ first
    if args.basic is not None or args.perf is not None or args.test_all or args.clean:
        clean_directory(test_output_base)
    passed = 0
    failed = 0
    selected_tests_count = len(test_files)
    excluded_count = len(args.exclude) if args.exclude else 0
    interrupted = False

    try:
        for test_file in test_files:
            filename = os.path.basename(test_file)
            name_no_ext = os.path.splitext(filename)[0]
            print(f"Testing {name_no_ext}...")

            work_test_output_dir = os.path.join(test_output_base, "_work", name_no_ext)
            if os.path.exists(work_test_output_dir):
                shutil.rmtree(work_test_output_dir)
            os.makedirs(work_test_output_dir, exist_ok=True)

            # Prepare directories
            clean_directory(logs_dir)
            clean_directory(graphs_dir)
            clean_directory(dump_llvm_dir)
            clean_directory(dump_asm_dir)
            
            # Expected output file (if any)
            # We specify an output file in CWD, then move it.
            output_file_name = f"{name_no_ext}.asm" if args.qemu else f"{name_no_ext}.out"
            linked_ll_name = f"{name_no_ext}.linked.ll"
            target_dir = os.path.join(work_test_output_dir, "target")
            graph_output_dir = os.path.join(work_test_output_dir, "graph")
            os.makedirs(target_dir, exist_ok=True)
            os.makedirs(graph_output_dir, exist_ok=True)
            linked_ll_path = os.path.join(target_dir, linked_ll_name)
            if os.path.exists(linked_ll_path):
                os.unlink(linked_ll_path)
            
            # Run compiler
            # Command: ./target/debug/compiler <input> -o <output>
            cmd = [compiler_binary, test_file, "-o", output_file_name]
            if need_emit_llvm:
                cmd.append("--emit-llvm")
            if args.dump_llvm_after:
                cmd.append(f"--dump-llvm-after={args.dump_llvm_after}")
            if args.dump_asm_after:
                cmd.append(f"--dump-asm-after={args.dump_asm_after}")
            if args.graph:
                cmd.append("--graph")
            try:
                run_env = {**os.environ, "RUST_BACKTRACE": "1"} if args.trace else None

                if args.gdb:
                    debug_cmd = ["rust-gdb", "-tui", "--args", *cmd]
                    print(f"  [DEBUG] Launching: {' '.join(debug_cmd)}")
                    result = subprocess.run(debug_cmd, env=run_env)
                    final_returncode = result.returncode
                    # debugger runs interactively; output is shown directly in terminal.
                    final_stdout = b""
                    final_stderr = b""
                elif args.lldb:
                    debug_cmd = ["rust-lldb", "--", *cmd]
                    print(f"  [DEBUG] Launching: {' '.join(debug_cmd)}")
                    result = subprocess.run(debug_cmd, env=run_env)
                    final_returncode = result.returncode
                    # debugger runs interactively; output is shown directly in terminal.
                    final_stdout = b""
                    final_stderr = b""
                else:
                    result = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=run_env)
                    final_returncode = result.returncode
                    final_stdout = result.stdout
                    final_stderr = result.stderr
                runtime_with_ret = None
                runtime_raw = None

                if final_returncode == 0 and need_emit_llvm:
                    generated_ll = os.path.join(dump_llvm_dir, f"{name_no_ext}.ll")
                    if not os.path.exists(generated_ll):
                        final_returncode = 1
                        final_stderr += (
                            f"\n[ERROR] Generated LLVM IR not found: {generated_ll}\n"
                        ).encode()
                    else:
                        if args.graph:
                            graph_code, graph_stdout, graph_stderr = generate_cfg_graphs(
                                generated_ll,
                                graph_output_dir,
                                name_no_ext,
                            )
                            final_stdout += graph_stdout
                            final_stderr += graph_stderr
                            if graph_code != 0:
                                final_returncode = graph_code

                        if final_returncode == 0 and need_runtime_exec and not os.path.exists(sylib_ll):
                            final_returncode = 1
                            final_stderr += (
                                f"\n[ERROR] Runtime library LLVM IR not found: {sylib_ll}\n"
                            ).encode()

                        if final_returncode == 0 and need_runtime_exec:
                            link_cmd = ["llvm-link", generated_ll, sylib_ll, "-S", "-o", linked_ll_path]
                            link_result = subprocess.run(link_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                            final_stdout += link_result.stdout
                            final_stderr += link_result.stderr

                            if link_result.returncode != 0:
                                final_returncode = link_result.returncode
                        if final_returncode == 0 and need_runtime_exec:
                            input_file = os.path.splitext(test_file)[0] + ".in"
                            if exec_mode == 'lli':
                                lli_cmd = ["lli", linked_ll_path]
                                if os.path.exists(input_file):
                                    with open(input_file, "rb") as f_in:
                                        exec_result = subprocess.run(
                                            lli_cmd,
                                            stdin=f_in,
                                            stdout=subprocess.PIPE,
                                            stderr=subprocess.PIPE,
                                        )
                                else:
                                    exec_result = subprocess.run(
                                        lli_cmd,
                                        stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE,
                                    )
                            else:
                                obj_path = os.path.join(target_dir, f"{name_no_ext}.o")
                                asm_path = os.path.join(target_dir, f"{name_no_ext}.s")
                                exe_path = os.path.join(target_dir, f"{name_no_ext}.out")

                                llc_asm_cmd = ["llc", linked_ll_path, "-filetype=asm", "-o", asm_path]
                                llc_asm_result = subprocess.run(llc_asm_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                                final_stdout += llc_asm_result.stdout
                                final_stderr += llc_asm_result.stderr

                                if llc_asm_result.returncode != 0:
                                    final_returncode = llc_asm_result.returncode
                                    exec_result = None
                                else:
                                    llc_obj_cmd = ["llc", linked_ll_path, "-filetype=obj", "-o", obj_path]
                                    llc_obj_result = subprocess.run(llc_obj_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                                    final_stdout += llc_obj_result.stdout
                                    final_stderr += llc_obj_result.stderr

                                    if llc_obj_result.returncode != 0:
                                        final_returncode = llc_obj_result.returncode
                                        exec_result = None

                                    linker = shutil.which("clang") or shutil.which("cc") or shutil.which("gcc")
                                    if linker is None:
                                        final_returncode = 1
                                        final_stderr += b"\n[ERROR] No system linker found (clang/cc/gcc).\n"
                                        exec_result = None
                                    else:
                                        link_exe_cmd = [linker, obj_path, "-o", exe_path, "-no-pie"]
                                        link_exe_result = subprocess.run(
                                            link_exe_cmd,
                                            stdout=subprocess.PIPE,
                                            stderr=subprocess.PIPE,
                                        )
                                        final_stdout += link_exe_result.stdout
                                        final_stderr += link_exe_result.stderr

                                        if link_exe_result.returncode != 0:
                                            final_returncode = link_exe_result.returncode
                                            exec_result = None
                                        else:
                                            if os.path.exists(input_file):
                                                with open(input_file, "rb") as f_in:
                                                    exec_result = subprocess.run(
                                                        [exe_path],
                                                        stdin=f_in,
                                                        stdout=subprocess.PIPE,
                                                        stderr=subprocess.PIPE,
                                                    )
                                            else:
                                                exec_result = subprocess.run(
                                                    [exe_path],
                                                    stdout=subprocess.PIPE,
                                                    stderr=subprocess.PIPE,
                                                )

                            if exec_result is None:
                                runtime_raw = b""
                                runtime_with_ret = None
                            else:
                                runtime_raw = exec_result.stdout
                                runtime_with_ret = runtime_raw
                                if not runtime_with_ret.endswith(b"\n"):
                                    runtime_with_ret += b"\n"
                                runtime_with_ret += f"{exec_result.returncode}\n".encode()

                                final_stdout += runtime_with_ret
                                final_stderr += exec_result.stderr

                            if final_returncode == 0 and runtime_with_ret is None:
                                final_returncode = 1
                                final_stderr += b"\n[ERROR] Runtime execution did not produce output.\n"

                expected_output_file = os.path.splitext(test_file)[0] + ".out"
                if final_returncode == 0 and need_runtime_exec:
                    if runtime_with_ret is None:
                        final_returncode = 1
                        final_stderr += b"\n[ERROR] Runtime output missing for comparison.\n"
                    elif not os.path.exists(expected_output_file):
                        final_returncode = 1
                        final_stderr += (
                            f"\n[ERROR] Expected output not found: {expected_output_file}\n"
                        ).encode()
                    else:
                        with open(expected_output_file, "rb") as f_exp:
                            expected_bytes = f_exp.read()
                        expected_norm = normalize_output_bytes(expected_bytes)
                        actual_with_ret_norm = normalize_output_bytes(runtime_with_ret)
                        actual_raw_norm = normalize_output_bytes(runtime_raw or b"")
                        if expected_norm != actual_with_ret_norm and expected_norm != actual_raw_norm:
                            final_returncode = 1
                            final_stderr += b"\n[ERROR] Output mismatch with expected .out\n"

                        with open(os.path.join(work_test_output_dir, "actual.out"), "wb") as f_actual:
                            f_actual.write(runtime_with_ret)
                
                test_output_dir = work_test_output_dir
                if not args.qemu:
                    # Determine output directory based on result
                    status = "passed" if final_returncode == 0 else "failed"
                    test_output_dir = os.path.join(test_output_base, status, name_no_ext)
                    
                    # Clean up the other possible location to avoid confusion
                    other_status = "failed" if final_returncode == 0 else "passed"
                    other_output_dir = os.path.join(test_output_base, other_status, name_no_ext)
                    if os.path.exists(other_output_dir):
                        shutil.rmtree(other_output_dir)

                    if os.path.exists(test_output_dir):
                        shutil.rmtree(test_output_dir)
                    os.makedirs(os.path.dirname(test_output_dir), exist_ok=True)

                # Copy original source file
                shutil.copy2(test_file, work_test_output_dir)

                if os.path.exists(expected_output_file):
                    shutil.copy2(
                        expected_output_file,
                        os.path.join(work_test_output_dir, "expected.out"),
                    )

                # Save stdout/stderr
                with open(os.path.join(work_test_output_dir, "stdout.txt"), "wb") as f:
                    f.write(final_stdout)
                with open(os.path.join(work_test_output_dir, "stderr.txt"), "wb") as f:
                    f.write(final_stderr)

                # Move logs
                if os.path.exists(logs_dir):
                    for f in os.listdir(logs_dir):
                        shutil.move(os.path.join(logs_dir, f), os.path.join(work_test_output_dir, f))
                
                # Move compiler-generated graphs (if any)
                if os.path.exists(graphs_dir):
                    for f in os.listdir(graphs_dir):
                        shutil.move(os.path.join(graphs_dir, f), os.path.join(work_test_output_dir, f))

                # Move dumped LLVM IR
                if os.path.exists(dump_llvm_dir):
                    for f in os.listdir(dump_llvm_dir):
                        src = os.path.join(dump_llvm_dir, f)
                        if f.endswith('.ll'):
                            dst = os.path.join(target_dir, f)
                        else:
                            dst = os.path.join(work_test_output_dir, f)
                        shutil.move(src, dst)

                # Move dumped assembly
                if os.path.exists(dump_asm_dir):
                    for f in os.listdir(dump_asm_dir):
                        src = os.path.join(dump_asm_dir, f)
                        if f.endswith('.asm'):
                            dst = os.path.join(target_dir, f)
                        else:
                            dst = os.path.join(work_test_output_dir, f)
                        shutil.move(src, dst)
                
                # Move output file
                if os.path.exists(output_file_name):
                    shutil.move(output_file_name, os.path.join(work_test_output_dir, output_file_name))

                if args.qemu:
                    asm_host_path = resolve_asm_artifact(work_test_output_dir, target_dir, name_no_ext)
                    if asm_host_path is None:
                        final_returncode = 1
                        asm_hint = [
                            f"target files: {', '.join(sorted(os.listdir(target_dir))) if os.path.isdir(target_dir) else '<missing target dir>'}",
                            f"work files: {', '.join(sorted(os.listdir(work_test_output_dir))) if os.path.isdir(work_test_output_dir) else '<missing work dir>'}",
                        ]
                        with open(os.path.join(work_test_output_dir, "stderr.txt"), "ab") as f_err:
                            f_err.write(
                                (
                                    f"\n[ERROR] Asm artifact not found for {name_no_ext}. "
                                    + " | ".join(asm_hint)
                                    + "\n"
                                ).encode()
                            )
                    elf_host_path = os.path.join(target_dir, f"{name_no_ext}.elf")
                    qemu_cases.append({
                        "name": name_no_ext,
                        "test_file": test_file,
                        "work_dir": work_test_output_dir,
                        "target_dir": target_dir,
                        "asm_path": asm_host_path,
                        "elf_path": elf_host_path,
                        "expected_output_file": expected_output_file,
                        "returncode": final_returncode,
                        "host_passed": final_returncode == 0,
                        "runtime_done": False,
                        "finalized": False,
                    })

                    if final_returncode != 0:
                        print(f"  [HOST COMPILATION FAILED] {name_no_ext} (Exit Code: {final_returncode})")
                    else:
                        print(f"  [HOST COMPILATION PASSED] {name_no_ext}")
                    continue

                if args.qemu_debug and final_returncode == 0:
                    asm_host_path = resolve_asm_artifact(work_test_output_dir, target_dir, name_no_ext)
                    if asm_host_path is None:
                        final_returncode = 1
                        with open(os.path.join(work_test_output_dir, "stderr.txt"), "ab") as f_err:
                            f_err.write(
                                f"\n[ERROR] Asm artifact not found for {name_no_ext}\n".encode()
                            )
                    else:
                        elf_host_path = os.path.join(target_dir, f"{name_no_ext}.elf")
                        print("  [QEMU DEBUG] Starting interactive gdb session (-tui) via gdb stub...")
                        start_result = start_qemu_container(qemu_image, qemu_container_name, repo_root)
                        if start_result.returncode != 0:
                            final_returncode = 1
                            with open(os.path.join(work_test_output_dir, "stderr.txt"), "ab") as f_err:
                                f_err.write(
                                    b"\n[ERROR] Failed to start qemu docker container for debug session\n"
                                )
                                f_err.write(start_result.stderr)
                        else:
                            try:
                                debug_code = run_qemu_gdb_debug(
                                    qemu_container_name,
                                    repo_root,
                                    asm_host_path,
                                    elf_host_path,
                                )
                                if debug_code != 0:
                                    final_returncode = debug_code
                            finally:
                                stop_qemu_container(qemu_container_name)

                shutil.move(work_test_output_dir, test_output_dir)

                if final_returncode != 0:
                    print(f"  [FAILED] {name_no_ext} (Exit Code: {final_returncode})")
                    failed += 1
                else:
                    print(f"  [PASSED] {name_no_ext}")
                    passed += 1

            except Exception as e:
                print(f"  [ERROR] Exception during test {name_no_ext}: {e}")

        if args.qemu:
            runnable_cases = [case for case in qemu_cases if case["returncode"] == 0]
            container_running = False

            if runnable_cases:
                print("Starting qemu user-mode container...")
                start_result = start_qemu_container(qemu_image, qemu_container_name, repo_root)
                if start_result.returncode != 0:
                    error_msg = (
                        b"\n[ERROR] Failed to start qemu docker container\n"
                        + start_result.stderr
                    )
                    print("Failed to start qemu container.")
                    for case in runnable_cases:
                        case["returncode"] = 1
                        case["runtime_done"] = True
                        with open(os.path.join(case["work_dir"], "stderr.txt"), "ab") as f_err:
                            f_err.write(error_msg)
                else:
                    container_running = True
                    try:
                        for case in runnable_cases:
                            name_no_ext = case["name"]
                            print(f"  [QEMU] Running {name_no_ext}...")
                            input_file = os.path.splitext(case["test_file"])[0] + ".in"
                            qemu_code, qemu_stdout, qemu_stderr, runtime_with_ret, runtime_raw = run_qemu_runtime(
                                qemu_container_name,
                                repo_root,
                                case["asm_path"],
                                case["elf_path"],
                                input_file,
                            )

                            with open(os.path.join(case["work_dir"], "stdout.txt"), "ab") as f_out:
                                f_out.write(qemu_stdout)
                            with open(os.path.join(case["work_dir"], "stderr.txt"), "ab") as f_err:
                                f_err.write(qemu_stderr)

                            if runtime_with_ret is None:
                                case["returncode"] = 1
                                case["runtime_done"] = True
                                with open(os.path.join(case["work_dir"], "stderr.txt"), "ab") as f_err:
                                    f_err.write(b"\n[ERROR] qemu runtime execution did not produce output.\n")
                                continue

                            expected_output_file = case["expected_output_file"]
                            if not os.path.exists(expected_output_file):
                                case["returncode"] = 1
                                case["runtime_done"] = True
                                with open(os.path.join(case["work_dir"], "stderr.txt"), "ab") as f_err:
                                    f_err.write(
                                        f"\n[ERROR] Expected output not found: {expected_output_file}\n".encode()
                                    )
                                continue

                            with open(expected_output_file, "rb") as f_exp:
                                expected_bytes = f_exp.read()

                            expected_norm = normalize_output_bytes(expected_bytes)
                            actual_with_ret_norm = normalize_output_bytes(runtime_with_ret)
                            actual_raw_norm = normalize_output_bytes(runtime_raw or b"")
                            if expected_norm != actual_with_ret_norm and expected_norm != actual_raw_norm:
                                case["returncode"] = 1
                                with open(os.path.join(case["work_dir"], "stderr.txt"), "ab") as f_err:
                                    f_err.write(b"\n[ERROR] Output mismatch with expected .out\n")
                            else:
                                case["returncode"] = 0

                            case["runtime_done"] = True

                            with open(os.path.join(case["work_dir"], "actual.out"), "wb") as f_actual:
                                f_actual.write(runtime_with_ret)

                            # Keep qemu process return code in logs for debugging; pass/fail is decided by output diff.
                            with open(os.path.join(case["work_dir"], "stderr.txt"), "ab") as f_err:
                                f_err.write(f"\n[INFO] qemu exit code: {qemu_code}\n".encode())
                    finally:
                        if container_running:
                            stop_qemu_container(qemu_container_name)

            moved_passed, moved_failed = finalize_qemu_cases(qemu_cases, test_output_base)
            passed += moved_passed
            failed += moved_failed

    except KeyboardInterrupt:
        interrupted = True
        print("Test interrupted by user. Finalizing finished results...")
        if args.qemu:
            moved_passed, moved_failed = finalize_qemu_cases(qemu_cases, test_output_base)
            passed += moved_passed
            failed += moved_failed

    skipped_count = excluded_count + max(selected_tests_count - (passed + failed), 0)
    print(f"Testing complete. Passed: {passed}, Failed: {failed}, Skipped: {skipped_count}")

    if interrupted:
        sys.exit(1)


if __name__ == "__main__":
    main()
