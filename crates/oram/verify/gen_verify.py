#!/usr/bin/env python3
# Copyright 2026 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     https://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Differentially private ORAM constant-time and data-obliviousness verification using Binsec/Rel.

Uses native ELF parsing via pyelftools instead of invoking external CLI tools.
"""

import argparse
import json
import os
import struct
import subprocess
import sys
import time

# Support both pip-installed packages and local third_party directory
try:
  from elftools.elf.elffile import ELFFile
except ImportError:
  for levels in range(1, 10):
    candidate = os.path.abspath(
        os.path.join(
            os.path.dirname(__file__), *([".."] * levels), "third_party/py"
        )
    )
    if os.path.isdir(candidate) and candidate not in sys.path:
      sys.path.insert(0, candidate)
  from elftools.elf.elffile import ELFFile


def parse_binary(binary_path):
  """Parses binary symbols, GOT table, and function code using pyelftools."""
  with open(binary_path, "rb") as f:
    elf = ELFFile(f)

    symtab = elf.get_section_by_name(".symtab")
    if not symtab:
      raise ValueError(
          "Binary has no .symtab section. Build with debuginfo/symbols."
      )

    verify_funcs = {}
    secret_globals = []
    alloc_symbols = []
    heap_ptr_addr = None
    symbol_by_addr = {}

    alloc_keywords = [
        "__rust_alloc",
        "__rust_dealloc",
        "__rust_realloc",
        "__rust_alloc_zeroed",
    ]

    for sym in symtab.iter_symbols():
      name = sym.name
      addr = sym["st_value"]
      size = sym["st_size"]
      if addr:
        symbol_by_addr[addr] = name

      if name.startswith("verify_"):
        verify_funcs[name] = (addr, size)
      elif name.startswith("SECRET_") or name.startswith("OUT_"):
        secret_globals.append(name)
      elif name == "HEAP_PTR":
        heap_ptr_addr = addr
      else:
        for kw in alloc_keywords:
          if kw in name:
            alloc_symbols.append((name, kw, addr))
            break

    # Parse GOT
    got_sec = elf.get_section_by_name(".got")
    got_entries = []
    if got_sec:
      got_data = got_sec.data()
      got_addr = got_sec["sh_addr"]
      for offset in range(0, len(got_data), 8):
        val = struct.unpack("<Q", got_data[offset : offset + 8])[0]
        if val != 0:
          got_entries.append({"addr": got_addr + offset, "value": val})

    # Parse Text section for instruction scanning
    text_sec = elf.get_section_by_name(".text")
    text_addr = text_sec["sh_addr"]
    text_data = text_sec.data()

    func_offsets = {}
    for func_name, (func_addr, func_size) in verify_funcs.items():
      rets = []
      ud2s = []
      try:
        res = subprocess.run(
            ["objdump", "-d", f"--disassemble={func_name}", binary_path],
            capture_output=True,
            text=True,
            check=True,
        )
        for line in res.stdout.splitlines():
          line = line.strip()
          if ":" in line:
            parts = line.split(":", 1)
            try:
              addr = int(parts[0], 16)
              asm = parts[1].split("#")[0].strip()
              if asm.endswith("\tret") or asm.endswith(" ret") or asm == "ret":
                rets.append(addr)
              elif (
                  asm.endswith("\tud2") or asm.endswith(" ud2") or asm == "ud2"
              ):
                ud2s.append(addr)
            except ValueError:
              pass
      except Exception:
        offset = func_addr - text_addr
        code = text_data[offset : offset + func_size]
        rets = [func_addr + i for i, b in enumerate(code) if b == 0xC3]
        ud2s = [
            func_addr + i
            for i in range(len(code) - 1)
            if code[i] == 0x0F and code[i + 1] == 0x0B
        ]
      func_offsets[func_name] = (rets, ud2s)

    return (
        verify_funcs,
        secret_globals,
        alloc_symbols,
        heap_ptr_addr,
        symbol_by_addr,
        got_entries,
        func_offsets,
    )


def generate_got_init(
    got_entries, heap_ptr_addr, symbol_by_addr, alloc_symbols
):
  """Generate binsec directives to initialize GOT entries, filtering to only needed ones."""
  lines = ["# GOT initialization (filtered)"]

  needed_addrs = set()
  if heap_ptr_addr:
    needed_addrs.add(heap_ptr_addr)

  for mangled, _, addr in alloc_symbols:
    needed_addrs.add(addr)

  if got_entries:
    for entry in got_entries:
      val = entry["value"]
      sym_name = symbol_by_addr.get(val, "")

      keep = False
      if val in needed_addrs:
        keep = True
      elif (
          sym_name.startswith("SECRET_")
          or sym_name.startswith("OUT_")
          or sym_name == "HEAP_PTR"
      ):
        keep = True
      elif any(
          sym_name == m or sym_name.startswith(m + "@")
          for m in ["memset", "memcpy", "memmove", "memcmp"]
      ):
        keep = True

      if keep:
        comment = f" # points to {sym_name}" if sym_name else ""
        lines.append(f"@[{hex(entry['addr'])}, <-, 8] := {hex(val)}{comment}")
  lines.append("")

  if heap_ptr_addr:
    lines.append(
        "# Heap allocator state (initialized to 0x30000000 at global"
        f" {hex(heap_ptr_addr)})"
    )
    lines.append(f"@[{hex(heap_ptr_addr)}, <-, 8] := 0x30000000")
    lines.append("")
  return "\n".join(lines)


def generate_allocator_stubs(alloc_symbols, heap_ptr_addr):
  """Generate binsec replace directives for allocator symbols."""
  stubs = []
  addr_ptr = hex(heap_ptr_addr) if heap_ptr_addr else "0x20000000"

  for mangled, demangled, addr in alloc_symbols:
    is_dealloc = "dealloc" in demangled or "free" in demangled.lower()
    is_realloc = "realloc" in demangled

    if is_dealloc:
      stub = f"replace <{mangled}> (ptr) by\n  return\nend"
    elif is_realloc:
      stub = (
          f"replace <{mangled}> (ptr, old_size, align, new_size) by\n"
          f"  rax := mem[{addr_ptr}, 8]\n"
          "  rcx := align - 1\n"
          "  rdx := 0 - align\n"
          "  rcx := rax + rcx\n"
          "  rax := rcx & rdx\n"
          f"  mem[{addr_ptr}, 8] := rax + new_size\n"
          "  return\n"
          "end"
      )
    else:  # alloc / alloc_zeroed
      stub = (
          f"replace <{mangled}> (size, align) by\n"
          f"  rax := mem[{addr_ptr}, 8]\n"
          "  rcx := align - 1\n"
          "  rdx := 0 - align\n"
          "  rcx := rax + rcx\n"
          "  rax := rcx & rdx\n"
          f"  mem[{addr_ptr}, 8] := rax + size\n"
          "  return\n"
          "end"
      )

    stubs.append(f"# Stub: {demangled} @ {hex(addr)}\n{stub}\n")

  return "\n".join(stubs)


def generate_ini(
    func_name, func_addr, rets, ud2s, secret_globals, alloc_stubs, got_init
):
  """Generate a binsec .ini script for a verification function."""
  lines = [
      f"starting from <{func_name}>",
      "",
      "with concrete stack pointer",
      "",
  ]
  if got_init:
    lines.append(got_init)
    lines.append("")

  for g in secret_globals:
    if g.startswith("SECRET_"):
      lines.append(f"secret global {g}")
  lines.append("")

  halt_offsets = []
  if rets:
    halt_offsets.append(f"<{func_name}> + {hex(rets[-1] - func_addr)}")
  if ud2s:
    halt_offsets.append(f"<{func_name}> + {hex(ud2s[-1] - func_addr)}")

  if halt_offsets:
    lines.append(f"halt at {halt_offsets[0]}")

  lines.append("")
  if alloc_stubs:
    lines.append("# Allocator stubs (auto-generated)")
    lines.append(alloc_stubs)

  lines.append("explore all\n")
  return "\n".join(lines)


def main():
  parser = argparse.ArgumentParser(
      description=(
          "Auto-generate binsec .ini scripts and run CT verification using"
          " pyelftools"
      )
  )
  parser.add_argument(
      "binary", help="Path to the compiled verification harness binary"
  )
  parser.add_argument(
      "--output-dir", default=".", help="Directory to write .ini files"
  )
  parser.add_argument(
      "--output-json",
      default=None,
      help="Path to write JSON verification results summary",
  )
  parser.add_argument(
      "--run", action="store_true", help="Run binsec verification"
  )
  parser.add_argument(
      "--write-files", action="store_true", help="Write .ini files to disk"
  )
  parser.add_argument(
      "--timeout",
      type=int,
      default=120,
      help="Timeout in seconds per function verification",
  )
  parser.add_argument(
      "--verbose", action="store_true", help="Print verbose output"
  )
  args = parser.parse_args()

  write_files = args.write_files or not args.run
  binary = os.path.abspath(args.binary)
  if not os.path.isfile(binary):
    print(f"Error: binary not found: {binary}", file=sys.stderr)
    sys.exit(1)

  output_dir = os.path.abspath(args.output_dir)
  os.makedirs(output_dir, exist_ok=True)

  print("Parsing binary symbols, GOT, and text via pyelftools...")
  (
      verify_funcs,
      secret_globals,
      alloc_symbols,
      heap_ptr_addr,
      symbol_by_addr,
      got_entries,
      func_offsets,
  ) = parse_binary(binary)

  if not verify_funcs:
    print("Error: no verify_* functions found in binary!", file=sys.stderr)
    sys.exit(1)

  print(
      f"  Found {len(verify_funcs)} verification functions:"
      f" {', '.join(sorted(verify_funcs.keys()))}"
  )
  print(
      f"  Found {len(secret_globals)} secret globals:"
      f" {', '.join(secret_globals)}"
  )
  print(f"  Found {len(alloc_symbols)} allocator symbols")
  if heap_ptr_addr:
    print(f"  Found HEAP_PTR @ {hex(heap_ptr_addr)}")
  print(f"  Found {len(got_entries)} non-zero GOT entries")

  alloc_stubs = generate_allocator_stubs(alloc_symbols, heap_ptr_addr)
  got_init = generate_got_init(
      got_entries, heap_ptr_addr, symbol_by_addr, alloc_symbols
  )

  generated_ini_contents = {}
  for func_name, (func_addr, func_size) in sorted(verify_funcs.items()):
    rets, ud2s = func_offsets.get(func_name, ([], []))
    if not rets and not ud2s:
      print(f"  WARNING: No ret/ud2 found for {func_name}, skipping")
      continue

    ini_content = generate_ini(
        func_name,
        func_addr,
        rets,
        ud2s,
        secret_globals,
        alloc_stubs,
        got_init,
    )
    generated_ini_contents[func_name] = ini_content

    if write_files:
      ini_path = os.path.join(output_dir, f"{func_name}.ini")
      with open(ini_path, "w") as f:
        f.write(ini_content)
      if args.verbose:
        print(f"  Written: {ini_path}")

  if args.run:
    binsec_path = os.path.expanduser("~/bin/binsec")
    if not os.path.isfile(binsec_path):
      which_binsec = subprocess.run(
          ["which", "binsec"], capture_output=True, text=True
      ).stdout.strip()
      if which_binsec:
        binsec_path = which_binsec
      else:
        print("Error: binsec executable not found!", file=sys.stderr)
        sys.exit(1)

    print("\n" + "=" * 115)
    print("Running Binsec/Rel verification...")
    print("=" * 115)

    header = (
        f"{'Function':<36} | {'Status':<10} | {'Paths':<6} |"
        f" {'Instr (unrolled)':<16} | {'CF Checks':<10} | {'Mem Checks':<10} |"
        f" {'Time (s)':<8}"
    )
    print(header)
    print("-" * len(header))

    results = []

    for func_name, ini_content in sorted(generated_ini_contents.items()):
      depth = (
          10000
          if any(
              x in func_name
              for x in [
                  "write_to_path",
                  "merge_accumulate",
                  "insert_block",
                  "shard_routing",
              ]
          )
          else 1000
      )
      ini_path = (
          os.path.join(output_dir, f"{func_name}.ini")
          if write_files
          else "/dev/stdin"
      )

      cmd = [
          binsec_path,
          "-sse",
          "-checkct",
          "-sse-depth",
          str(depth),
          "-sse-script",
          ini_path,
          binary,
      ]

      t0 = time.time()
      try:
        res = subprocess.run(
            cmd,
            input=ini_content if not write_files else None,
            capture_output=True,
            text=True,
            timeout=args.timeout,
        )
        elapsed = time.time() - t0
        out = res.stdout
      except subprocess.TimeoutExpired:
        elapsed = time.time() - t0
        out = "TIMEOUT"

      status = "UNKNOWN"
      if (
          "Program status: safe" in out
          or "Program status is : safe" in out
          or "Program status: secure" in out
          or "Program status is : secure" in out
      ):
        status = "SAFE"
      elif (
          "Program status: insecure" in out
          or "Program status is : insecure" in out
      ):
        status = "INSECURE"
      elif "Program status is : unknown" in out:
        status = "SAFE*" if "checks pass" in out else "UNKNOWN"

      paths = "N/A"
      instr_unrolled = "N/A"
      cf_checks = "N/A"
      mem_checks = "N/A"

      for line in out.splitlines():
        line = line.strip()
        if "total paths" in line:
          paths = line.split()[-1]
        elif "visited instructions (unrolled)" in line:
          instr_unrolled = line.split()[-1]
        elif "control flow checks pass" in line:
          cf_checks = (
              line.split("control flow checks pass")[0]
              .split("]")[-1]
              .strip()
              .replace(" ", "")
          )
        elif "memory access checks pass" in line:
          mem_checks = (
              line.split("memory access checks pass")[0]
              .split("]")[-1]
              .strip()
              .replace(" ", "")
          )

      print(
          f"{func_name:<36} | {status:<10} | {paths:<6} | {instr_unrolled:<16}"
          f" | {cf_checks:<10} | {mem_checks:<10} | {elapsed:<8.3f}"
      )
      results.append({
          "function": func_name,
          "status": status,
          "paths": paths,
          "instr_unrolled": instr_unrolled,
          "cf_checks": cf_checks,
          "mem_checks": mem_checks,
          "elapsed_s": elapsed,
      })

    if args.output_json:
      os.makedirs(
          os.path.dirname(os.path.abspath(args.output_json)), exist_ok=True
      )
      with open(args.output_json, "w") as f:
        json.dump(results, f, indent=2)
      print(f"\nSaved verification results summary to {args.output_json}")

  return 0


if __name__ == "__main__":
  sys.exit(main())
