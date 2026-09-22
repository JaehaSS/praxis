#!/usr/bin/env python3
import asyncio
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
FORWARDER = ROOT / "scripts/workflow/egress-forwarder.py"


class ForwarderTest(unittest.TestCase):
    def test_forwards_connect_and_delivers_prompt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            socket = root / "proxy.sock"
            prompt = root / "prompt.txt"
            prompt.write_text("PRAXIS_WORKFLOW_CAPABILITY_OK", encoding="utf-8")
            vendor = root / "vendor.py"
            vendor.write_text(textwrap.dedent("""
                import socket, sys
                client = socket.create_connection(('127.0.0.1', 18080), timeout=3)
                client.sendall(b'CONNECT api.vendor.example:443 HTTP/1.1\\r\\nHost: api.vendor.example:443\\r\\n\\r\\n')
                assert b'200' in client.recv(128)
                client.sendall(b'ping')
                assert client.recv(4) == b'ping'
                assert sys.stdin.read() == 'PRAXIS_WORKFLOW_CAPABILITY_OK'
                print('PRAXIS_WORKFLOW_CAPABILITY_OK')
            """), encoding="utf-8")
            received = []

            async def handler(reader, writer):
                headers = await reader.readuntil(b"\r\n\r\n")
                received.append(headers)
                writer.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                await writer.drain()
                payload = await reader.readexactly(4)
                writer.write(payload)
                await writer.drain()
                writer.close()

            async def run():
                server = await asyncio.start_unix_server(handler, path=str(socket))
                process = await asyncio.create_subprocess_exec(
                    sys.executable, str(FORWARDER), "--contract", "praxis-egress-forwarder-v1", "--socket", str(socket),
                    "--listen", "127.0.0.1:18080", "--prompt", str(prompt), "--", sys.executable, str(vendor),
                    stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
                )
                stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=10)
                server.close(); await server.wait_closed()
                return process.returncode, stdout, stderr

            code, stdout, stderr = asyncio.run(run())
            self.assertEqual(code, 0, stderr.decode())
            self.assertIn(b"PRAXIS_WORKFLOW_CAPABILITY_OK", stdout)
            self.assertEqual(len(received), 1)
            self.assertTrue(received[0].startswith(b"CONNECT api.vendor.example:443"))

    def test_sigterm_reaps_vendor_process_group(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            socket = root / "proxy.sock"
            prompt = root / "prompt.txt"; prompt.write_text("x")
            vendor = root / "vendor.py"
            vendor.write_text("import time\ntime.sleep(60)\n", encoding="utf-8")
            process = subprocess.Popen([sys.executable, str(FORWARDER), "--contract", "praxis-egress-forwarder-v1", "--socket", str(socket), "--listen", "127.0.0.1:18080", "--prompt", str(prompt), "--", sys.executable, str(vendor)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            time.sleep(0.3)
            process.send_signal(signal.SIGTERM)
            _, stderr = process.communicate(timeout=8)
            self.assertNotEqual(process.returncode, 0, stderr.decode())


if __name__ == "__main__":
    unittest.main()
