import socket, json, struct, sys

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect('/home/wec/scavenger/.scavenger/daemon.sock')

req = json.dumps({'method': 'capsule', 'file': 'src/main.rs'}).encode()
sock.sendall(struct.pack('>I', len(req)) + req)

len_buf = sock.recv(4)
resp_len = struct.unpack('>I', len_buf)[0]

data = b''
while len(data) < resp_len:
    chunk = sock.recv(min(4096, resp_len - len(data)))
    if not chunk:
        break
    data += chunk
sock.close()

resp = json.loads(data)
if 'capsule' in resp:
    print(resp['capsule'])
else:
    print(json.dumps(resp, indent=2))
