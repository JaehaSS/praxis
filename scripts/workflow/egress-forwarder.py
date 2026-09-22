#!/usr/bin/env python3
"""Container-side half of the Praxis egress-forwarder-v1 contract.

The runner mounts a per-attempt Unix socket connected to the host CONNECT
proxy. This program exposes it only on loopback, launches the image-registered
vendor executable with the prompt on stdin, and reaps its process group. It
never interprets an objective as shell syntax and never contacts a vendor by
itself.
"""

import argparse
import asyncio
import os
import signal
import sys
from contextlib import suppress


MAX_HEADER_BYTES = 16 * 1024
MAX_TUNNELS = 16
MAX_PROMPT_BYTES = 1024 * 1024


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--contract", required=True)
    parser.add_argument("--socket", required=True)
    parser.add_argument("--listen", required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("vendor", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.contract != "praxis-egress-forwarder-v1":
        parser.error("unsupported forwarder contract")
    if not args.vendor or args.vendor[0] != "--" or len(args.vendor) < 2:
        parser.error("vendor executable must follow --")
    args.vendor = args.vendor[1:]
    host, separator, port = args.listen.rpartition(":")
    if separator != ":" or host != "127.0.0.1" or not port.isdecimal() or not 1 <= int(port) <= 65535:
        parser.error("listen must be a 127.0.0.1 TCP address")
    args.port = int(port)
    return args


async def read_headers(reader):
    data = bytearray()
    while len(data) < MAX_HEADER_BYTES:
        chunk = await reader.read(1)
        if not chunk:
            raise ValueError("client disconnected before CONNECT request")
        data.extend(chunk)
        if data.endswith(b"\r\n\r\n"):
            request = data.split(b"\r\n", 1)[0].decode("ascii", "strict").split()
            if len(request) != 3 or request[0] != "CONNECT" or request[2] not in ("HTTP/1.0", "HTTP/1.1"):
                raise ValueError("only a well-formed CONNECT request is allowed")
            return bytes(data)
    raise ValueError("CONNECT request headers exceed limit")


async def relay(source, destination):
    try:
        while True:
            data = await source.read(64 * 1024)
            if not data:
                with suppress(Exception):
                    destination.write_eof()
                return
            destination.write(data)
            await destination.drain()
    except (ConnectionError, asyncio.IncompleteReadError):
        return


async def serve_client(reader, writer, socket_path, semaphore, tunnels):
    # Do not accumulate an unbounded queue of waiting client tasks.
    if semaphore.locked():
        writer.write(b"HTTP/1.1 503 Too Many Connections\r\nConnection: close\r\n\r\n")
        await writer.drain()
        writer.close()
        await writer.wait_closed()
        return
    async with semaphore:
        try:
            headers = await read_headers(reader)
            upstream_reader, upstream_writer = await asyncio.open_unix_connection(socket_path)
            upstream_writer.write(headers)
            await upstream_writer.drain()
            # The host proxy owns CONNECT parsing, DNS validation and the 200.
            first = await asyncio.wait_for(upstream_reader.readuntil(b"\r\n\r\n"), timeout=15)
            if len(first) > MAX_HEADER_BYTES or not first.startswith(b"HTTP/1."):
                raise ValueError("invalid upstream proxy response")
            writer.write(first)
            await writer.drain()
            left = asyncio.create_task(relay(reader, upstream_writer))
            right = asyncio.create_task(relay(upstream_reader, writer))
            tunnels.update((left, right))
            done, pending = await asyncio.wait((left, right), return_when=asyncio.FIRST_COMPLETED)
            for task in pending:
                task.cancel()
            await asyncio.gather(*pending, return_exceptions=True)
            tunnels.difference_update((left, right))
        except (ValueError, OSError, asyncio.TimeoutError):
            with suppress(Exception):
                writer.write(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                await writer.drain()
        finally:
            writer.close()
            with suppress(Exception):
                await writer.wait_closed()


async def main():
    args = parse_args()
    with open(args.prompt, "rb") as prompt_file:
        prompt = prompt_file.read(MAX_PROMPT_BYTES + 1)
    if len(prompt) > MAX_PROMPT_BYTES:
        raise ValueError("prompt exceeds forwarder size limit")
    os.environ["PRAXIS_WORKFLOW_PROMPT_FILE"] = args.prompt
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for signum in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(signum, stop.set)
    semaphore = asyncio.Semaphore(MAX_TUNNELS)
    tunnels = set()
    server = await asyncio.start_server(
        lambda reader, writer: serve_client(reader, writer, args.socket, semaphore, tunnels),
        host="127.0.0.1", port=args.port,
    )
    # `start_new_session` lets termination kill every vendor descendant, while
    # the executable/argv remain the literal values registered in the profile.
    process = await asyncio.create_subprocess_exec(
        *args.vendor,
        stdin=asyncio.subprocess.PIPE,
        start_new_session=True,
    )
    process.stdin.write(prompt)
    await process.stdin.drain()
    process.stdin.close()
    wait_process = asyncio.create_task(process.wait())
    wait_stop = asyncio.create_task(stop.wait())
    done, pending = await asyncio.wait((wait_process, wait_stop), return_when=asyncio.FIRST_COMPLETED)
    if wait_stop in done and process.returncode is None:
        with suppress(ProcessLookupError):
            os.killpg(process.pid, signal.SIGTERM)
        try:
            await asyncio.wait_for(wait_process, timeout=5)
        except asyncio.TimeoutError:
            with suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGKILL)
            await wait_process
    for task in pending:
        task.cancel()
    server.close()
    await server.wait_closed()
    for task in tuple(tunnels):
        task.cancel()
    await asyncio.gather(*tuple(tunnels), return_exceptions=True)
    return process.returncode if process.returncode is not None else 1


if __name__ == "__main__":
    try:
        raise SystemExit(asyncio.run(main()))
    except KeyboardInterrupt:
        raise SystemExit(130)
