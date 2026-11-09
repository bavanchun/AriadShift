"""Owned loopback relay into an internal Docker network, with bounded connections.

Docker does not publish ports on internal bridge networks. Keeping the container
on that network and forwarding from the host preserves both isolation and the
fixed local client URL without adding an egress-capable second network.
"""

import select
import socket
import socketserver
import threading
import time


class LoopbackRelay:
    def __init__(self, address, timeout):
        self.sockets = set()
        self.lock = threading.Lock()
        self.stopping = threading.Event()
        owner = self

        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                peer = None
                try:
                    peer = socket.create_connection(address, timeout=2)
                    connections = (self.request, peer)
                    for connection in connections:
                        connection.settimeout(2)
                    with owner.lock:
                        owner.sockets.update(connections)
                    deadline = time.monotonic() + timeout
                    while not owner.stopping.is_set() and time.monotonic() < deadline:
                        ready, _, _ = select.select(connections, [], [], min(1, max(0, deadline - time.monotonic())))
                        for connection in ready:
                            content = connection.recv(65536)
                            if not content:
                                return
                            (peer if connection is self.request else self.request).sendall(content)
                except OSError:
                    pass  # The HTTP caller records connection failures as failed attempts.
                finally:
                    with owner.lock:
                        owner.sockets.difference_update((self.request, peer))
                    if peer:
                        peer.close()

        class Server(socketserver.ThreadingTCPServer):
            allow_reuse_address = True
            daemon_threads = True

        self.server = Server(("127.0.0.1", 18711), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.stopping.set()
        with self.lock:
            for connection in tuple(self.sockets):
                try:
                    connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                connection.close()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)
