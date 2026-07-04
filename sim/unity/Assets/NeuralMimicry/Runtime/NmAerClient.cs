// NmAerClient.cs — TCP client for AARNN neuromorphic bridge
// Protocol: AER1 binary framing over TCP with u32 LE length-prefix framing.
// Compatible with Unity 2022.3 LTS+.

using System;
using System.Collections.Generic;
using System.IO;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// Thread-safe TCP client for the AARNN AER bridge.
    /// Encodes sensor floats as AER1 spike trains and decodes motor-spike replies.
    /// Also supports a raw-float fallback for non-AER servers.
    /// </summary>
    public sealed class NmAerClient : IDisposable
    {
        // ------------------------------------------------------------------ //
        // AER constants
        // ------------------------------------------------------------------ //

        /// <summary>Magic bytes that open every AER1 packet.</summary>
        private static readonly byte[] AerMagic = { (byte)'A', (byte)'E', (byte)'R', (byte)'1' };

        /// <summary>
        /// AER address base for sensory channels. Must match the server's
        /// <c>--aer-sensory-base</c> (AARNN default: 4096). Sent addresses are
        /// <c>SensoryBase + channel_index</c>.
        /// </summary>
        public int SensoryBase = 4096;

        /// <summary>
        /// AER address base for motor/output channels. Must match the server's
        /// <c>--aer-output-base</c> (AARNN default: 16384). Received addresses are
        /// decoded as <c>address - OutputBase</c>.
        /// </summary>
        public int OutputBase = 16384;

        // ------------------------------------------------------------------ //
        // Connection state
        // ------------------------------------------------------------------ //

        private readonly string _host;
        private readonly int _port;
        private readonly float _spikeThreshold;
        private readonly bool _useAer;

        private TcpClient _tcp;
        private NetworkStream _stream;
        private readonly object _lock = new object();

        private bool _disposed;
        private bool _connected;

        // ------------------------------------------------------------------ //
        // Construction / teardown
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Creates an <see cref="NmAerClient"/> with the given connection parameters.
        /// Call <see cref="Connect"/> before stepping.
        /// </summary>
        /// <param name="host">AARNN server hostname or IP.</param>
        /// <param name="port">TCP port (default 7890).</param>
        /// <param name="spikeThreshold">
        /// Minimum normalised value [0,1] that triggers an AER spike event.
        /// Values at or below this are silent (no event emitted).
        /// </param>
        /// <param name="useAer">
        /// When <c>true</c> (default) use AER1 framing; when <c>false</c> use
        /// the raw-float wire format for servers that do not implement AER.
        /// </param>
        public NmAerClient(string host = "127.0.0.1", int port = 7890,
                           float spikeThreshold = 0.5f, bool useAer = true)
        {
            _host = host;
            _port = port;
            _spikeThreshold = spikeThreshold;
            _useAer = useAer;
        }

        // ------------------------------------------------------------------ //
        // Public API
        // ------------------------------------------------------------------ //

        /// <summary>Returns <c>true</c> when the underlying TCP connection is open.</summary>
        public bool IsConnected
        {
            get
            {
                lock (_lock)
                    return _connected && _tcp != null && _tcp.Connected;
            }
        }

        /// <summary>
        /// Opens the TCP connection to the AARNN server.
        /// Safe to call from any thread.
        /// </summary>
        /// <exception cref="SocketException">Thrown when the server cannot be reached.</exception>
        public void Connect()
        {
            lock (_lock)
            {
                DropConnection();
                _tcp = new TcpClient();
                _tcp.Connect(_host, _port);
                _tcp.NoDelay = true;
                _tcp.ReceiveTimeout = 1000; // 1 s read timeout
                _stream = _tcp.GetStream();
                _connected = true;
            }
        }

        /// <summary>
        /// Sends the JSON handshake that announces sensor and actuator names.
        /// Must be called once immediately after <see cref="Connect"/> and before
        /// the first <see cref="Step"/> call.
        /// </summary>
        /// <param name="sensorNames">Ordered list of sensor channel names.</param>
        /// <param name="actuatorNames">Ordered list of actuator channel names.</param>
        public void SendHandshake(string[] sensorNames, string[] actuatorNames)
        {
            if (sensorNames == null) throw new ArgumentNullException(nameof(sensorNames));
            if (actuatorNames == null) throw new ArgumentNullException(nameof(actuatorNames));

            // Build minimal JSON by hand — no external JSON dependency required.
            var sb = new StringBuilder();
            sb.Append("{\"s_names\":[");
            for (int i = 0; i < sensorNames.Length; i++)
            {
                if (i > 0) sb.Append(',');
                sb.Append('"');
                sb.Append(JsonEscape(sensorNames[i]));
                sb.Append('"');
            }
            sb.Append("],\"o_names\":[");
            for (int i = 0; i < actuatorNames.Length; i++)
            {
                if (i > 0) sb.Append(',');
                sb.Append('"');
                sb.Append(JsonEscape(actuatorNames[i]));
                sb.Append('"');
            }
            sb.Append("],\"sensory\":");
            sb.Append(sensorNames.Length);
            sb.Append(",\"output\":");
            sb.Append(actuatorNames.Length);
            sb.Append('}');

            byte[] payload = Encoding.UTF8.GetBytes(sb.ToString());
            WriteFramed(payload);
        }

        /// <summary>
        /// Sends one timestep of sensor values and reads back motor outputs.
        /// Reconnects automatically on disconnect.
        /// </summary>
        /// <param name="tMs">Current simulation time in milliseconds.</param>
        /// <param name="sensorValues">
        /// Normalised sensor readings in [0,1]. Length must match the count sent
        /// in <see cref="SendHandshake"/>.
        /// </param>
        /// <param name="outputBuffer">
        /// Pre-allocated output buffer. On success this is filled with normalised
        /// motor activations in [0,1]. Must be at least as large as the output
        /// channel count from the handshake.
        /// </param>
        /// <returns>
        /// <c>true</c> when a reply was received and <paramref name="outputBuffer"/>
        /// has been populated; <c>false</c> on a communication error (the caller
        /// should treat outputs as stale).
        /// </returns>
        public bool Step(float tMs, float[] sensorValues, float[] outputBuffer)
        {
            if (sensorValues == null) throw new ArgumentNullException(nameof(sensorValues));
            if (outputBuffer == null) throw new ArgumentNullException(nameof(outputBuffer));

            lock (_lock)
            {
                if (!_connected)
                {
                    TryReconnect();
                    if (!_connected) return false;
                }

                try
                {
                    byte[] request = _useAer
                        ? EncodeAer(tMs, sensorValues)
                        : EncodeRawFloats(tMs, sensorValues);

                    WriteFramed(request);

                    byte[] reply = ReadFramed();
                    if (reply == null || reply.Length == 0) return false;

                    if (_useAer)
                        DecodeAer(reply, outputBuffer);
                    else
                        DecodeRawFloats(reply, outputBuffer);

                    return true;
                }
                catch (Exception ex)
                {
                    Debug.LogWarning($"[NmAerClient] Step error: {ex.Message}. Dropping connection.");
                    DropConnection();
                    return false;
                }
            }
        }

        /// <inheritdoc/>
        public void Dispose()
        {
            lock (_lock)
            {
                if (_disposed) return;
                _disposed = true;
                DropConnection();
            }
        }

        // ------------------------------------------------------------------ //
        // AER encoding — floats → AER1 binary packet
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Encodes sensor values as an AER1 spike packet.
        /// Format: magic(4) + base_ts_us(u64 LE) + N * varint_event.
        /// Each event: delta_ts(varint) + addr(varint) + value(varint scaled 0..255).
        /// Only values above <see cref="_spikeThreshold"/> generate events.
        /// </summary>
        private byte[] EncodeAer(float tMs, float[] values)
        {
            ulong baseTs = (ulong)(tMs * 1000.0); // ms → µs

            using var ms = new MemoryStream(32 + values.Length * 4);

            // Magic
            ms.Write(AerMagic, 0, 4);

            // base_ts_us as u64 LE
            WriteU64Le(ms, baseTs);

            ulong prevTs = baseTs;
            for (int i = 0; i < values.Length; i++)
            {
                float v = values[i];
                if (v <= _spikeThreshold) continue;

                // delta_ts: 0 for all events in the same frame
                ulong deltaTs = 0;
                WriteVarint(ms, deltaTs);

                // addr: SensoryBase + channel index (matches server --aer-sensory-base)
                WriteVarint(ms, (ulong)(SensoryBase + i));

                // value: scaled to 0..255. The server treats any non-zero value as a
                // spike, so the exact magnitude is informational only.
                byte encoded = (byte)Mathf.RoundToInt(Mathf.Clamp01(v) * 255f);
                if (encoded == 0) encoded = 1; // guarantee an above-threshold value spikes
                WriteVarint(ms, encoded);
            }

            return ms.ToArray();
        }

        // ------------------------------------------------------------------ //
        // AER decoding — AER1 binary packet → float array
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Decodes an AER1 response packet into <paramref name="outputBuffer"/>.
        /// Clears the buffer first; missing channels remain 0.
        /// </summary>
        private void DecodeAer(byte[] data, float[] outputBuffer)
        {
            Array.Clear(outputBuffer, 0, outputBuffer.Length);

            if (data.Length < 12) return; // magic(4) + base_ts(8) minimum

            // Verify magic
            if (data[0] != 'A' || data[1] != 'E' || data[2] != 'R' || data[3] != '1')
                return;

            int pos = 12; // skip magic(4) + base_ts(8)

            while (pos < data.Length)
            {
                ulong deltaTs;
                if (!TryReadVarint(data, ref pos, out deltaTs)) break;

                ulong addr;
                if (!TryReadVarint(data, ref pos, out addr)) break;

                ulong val;
                if (!TryReadVarint(data, ref pos, out val)) break;

                // Motor addresses arrive as OutputBase + channel_index. Fall back to a
                // raw address for servers configured with an output base of 0.
                int idx = addr >= (ulong)OutputBase ? (int)(addr - (ulong)OutputBase) : (int)addr;
                // AARNN encodes output spikes with value == 1 (binary), so any non-zero
                // value is a full spike. Dividing by 255 here would collapse it to ~0.
                if (idx >= 0 && idx < outputBuffer.Length)
                    outputBuffer[idx] = (val & 0xFF) != 0 ? 1f : 0f;
            }
        }

        // ------------------------------------------------------------------ //
        // Raw-float format (fallback)
        // ------------------------------------------------------------------ //

        /// <summary>Encodes as: [f32 t_ms][f32 s0][f32 s1]...</summary>
        private static byte[] EncodeRawFloats(float tMs, float[] values)
        {
            using var ms = new MemoryStream((1 + values.Length) * 4);
            WriteF32Le(ms, tMs);
            foreach (float v in values)
                WriteF32Le(ms, v);
            return ms.ToArray();
        }

        /// <summary>Decodes as: [f32 o0][f32 o1]...</summary>
        private static void DecodeRawFloats(byte[] data, float[] outputBuffer)
        {
            Array.Clear(outputBuffer, 0, outputBuffer.Length);
            int count = Math.Min(data.Length / 4, outputBuffer.Length);
            for (int i = 0; i < count; i++)
                outputBuffer[i] = BitConverter.ToSingle(data, i * 4);
        }

        // ------------------------------------------------------------------ //
        // Length-prefix framing
        // ------------------------------------------------------------------ //

        /// <summary>Writes a u32 LE length prefix followed by <paramref name="payload"/>.</summary>
        private void WriteFramed(byte[] payload)
        {
            byte[] lenBytes = BitConverter.GetBytes((uint)payload.Length);
            if (!BitConverter.IsLittleEndian) Array.Reverse(lenBytes);
            _stream.Write(lenBytes, 0, 4);
            _stream.Write(payload, 0, payload.Length);
            _stream.Flush();
        }

        /// <summary>Reads a u32 LE length prefix, then reads exactly that many bytes.</summary>
        private byte[] ReadFramed()
        {
            byte[] lenBuf = ReadExact(4);
            if (lenBuf == null) return null;
            if (!BitConverter.IsLittleEndian) Array.Reverse(lenBuf);
            int length = (int)BitConverter.ToUInt32(lenBuf, 0);
            if (length == 0) return Array.Empty<byte>();
            return ReadExact(length);
        }

        /// <summary>Reads exactly <paramref name="count"/> bytes from the stream.</summary>
        private byte[] ReadExact(int count)
        {
            byte[] buf = new byte[count];
            int read = 0;
            while (read < count)
            {
                int n = _stream.Read(buf, read, count - read);
                if (n == 0) return null; // connection closed
                read += n;
            }
            return buf;
        }

        // ------------------------------------------------------------------ //
        // Varint helpers (unsigned LEB128)
        // ------------------------------------------------------------------ //

        private static void WriteVarint(Stream s, ulong value)
        {
            while (value >= 0x80)
            {
                s.WriteByte((byte)((value & 0x7F) | 0x80));
                value >>= 7;
            }
            s.WriteByte((byte)value);
        }

        private static bool TryReadVarint(byte[] data, ref int pos, out ulong result)
        {
            result = 0;
            int shift = 0;
            while (pos < data.Length)
            {
                byte b = data[pos++];
                result |= (ulong)(b & 0x7F) << shift;
                if ((b & 0x80) == 0) return true;
                shift += 7;
                if (shift >= 64) return false; // overflow guard
            }
            return false;
        }

        // ------------------------------------------------------------------ //
        // Primitive write helpers
        // ------------------------------------------------------------------ //

        private static void WriteU64Le(Stream s, ulong v)
        {
            byte[] b = BitConverter.GetBytes(v);
            if (!BitConverter.IsLittleEndian) Array.Reverse(b);
            s.Write(b, 0, 8);
        }

        private static void WriteF32Le(Stream s, float v)
        {
            byte[] b = BitConverter.GetBytes(v);
            if (!BitConverter.IsLittleEndian) Array.Reverse(b);
            s.Write(b, 0, 4);
        }

        // ------------------------------------------------------------------ //
        // Connection management
        // ------------------------------------------------------------------ //

        private void TryReconnect()
        {
            try
            {
                DropConnection();
                _tcp = new TcpClient();
                _tcp.Connect(_host, _port);
                _tcp.NoDelay = true;
                _tcp.ReceiveTimeout = 1000;
                _stream = _tcp.GetStream();
                _connected = true;
                Debug.Log($"[NmAerClient] Reconnected to {_host}:{_port}");
            }
            catch (Exception ex)
            {
                Debug.LogWarning($"[NmAerClient] Reconnect failed: {ex.Message}");
                _connected = false;
            }
        }

        private void DropConnection()
        {
            _connected = false;
            try { _stream?.Close(); } catch { /* ignored */ }
            try { _tcp?.Close(); } catch { /* ignored */ }
            _stream = null;
            _tcp = null;
        }

        // ------------------------------------------------------------------ //
        // Utility
        // ------------------------------------------------------------------ //

        private static string JsonEscape(string s)
        {
            return s.Replace("\\", "\\\\").Replace("\"", "\\\"")
                    .Replace("\n", "\\n").Replace("\r", "\\r").Replace("\t", "\\t");
        }
    }
}
