using System;
using System.Collections;
using System.IO;
using System.Text;
using UnityEngine;
using UnityEngine.Networking;

namespace NeuralMimicry
{
    /// <summary>Focused simulator conversation UI; the local proxy alone transduces/decodes neural I/O.</summary>
    public sealed class NmNaoChat : MonoBehaviour
    {
        [Serializable] public sealed class Settings { public string schema, url, adapter_token, join_token; public int body_port; }
        [Serializable] private sealed class Sample { public string player, text, modality = "typed", id; public long sequence, capture_ns; public float[] position; }
        [Serializable] private sealed class Reply { public string text, presentation_id, gesture; }
        [Serializable] private sealed class Result { public string schema, id, state, error; public Reply reply; }
        private NmNaoRobot robot;
        private Settings settings;
        private readonly string principal = "unity:" + Guid.NewGuid().ToString("N");
        private string input = "", status = "Start a local NAO social session to chat.", transcript = "", bubble = "";
        private int epoch;
        private long sequence;
        private bool busy, encountered;
        private float bubbleUntil;
        private UnityWebRequest pending;
        public float interactionRange = 8f;

        public static void Attach(NmNaoRobot nao)
        {
            var path = Environment.GetEnvironmentVariable("NM_NAO_SESSION_FILE");
            if (string.IsNullOrEmpty(path)) return;
            try
            {
                if (new FileInfo(path).Length > 16384) throw new InvalidDataException();
                var config = JsonUtility.FromJson<Settings>(File.ReadAllText(path));
                var uri = new Uri(config.url);
                if (config.schema != "NAO-SOCIAL/1" || uri.Scheme != "http" || uri.Host != "127.0.0.1" || !string.IsNullOrEmpty(uri.UserInfo)
                    || config.body_port < 1 || config.body_port > 65535 || config.adapter_token.Length < 24) throw new InvalidDataException();
                var ui = nao.gameObject.AddComponent<NmNaoChat>(); ui.robot = nao; ui.settings = config;
                // Clone the scene asset: never rewrite a persisted connector.
                nao.brainConnector = nao.brainConnector != null ? Instantiate(nao.brainConnector) : ScriptableObject.CreateInstance<NmBrainConnector>();
                nao.brainConnector.brainId = "nao"; nao.brainConnector.tcpHost = "127.0.0.1"; nao.brainConnector.tcpPort = config.body_port;
                nao.autoReconnect = false;
                ui.status = "Small social vocabulary. Try hello, name or help.";
            }
            catch (Exception) { Debug.LogWarning("NAO social configuration unavailable; no chat credential loaded."); }
        }
        private bool Nearby(Transform speaker) => robot != null && robot.IsConnected && speaker != null &&
            speaker.gameObject.scene == gameObject.scene && Vector3.Distance(speaker.position, transform.position) <= interactionRange;

        /// <summary>Host-side seam for an authenticated player/NPC in this scene. It grants no brain management.</summary>
        public bool Submit(Transform speaker, string text)
        {
            bool encounter = text == null;
            if (settings == null || busy || !Nearby(speaker)) { status = "Move within range of a connected NAO."; return false; }
            if (!encounter && (string.IsNullOrWhiteSpace(text) || Encoding.UTF8.GetByteCount(text) > 256 || Array.Exists(text.ToCharArray(), char.IsControl)))
            { status = "Use 1–256 UTF-8 bytes without control characters."; return false; }
            var relative = (speaker.position - transform.position) / interactionRange;
            var sample = new Sample { player = principal, text = text, sequence = sequence++, capture_ns = (long)(Time.realtimeSinceStartupAsDouble * 1e9),
                position = new[] { relative.x, relative.y, relative.z } };
            busy = true; encountered = true; transcript = encounter ? "NAO noticed you." : "You: " + text; StartCoroutine(Turn(sample, speaker, epoch)); return true;
        }
        private IEnumerator Request(string path, Sample body, Action<Result> done)
        {
            using (var request = new UnityWebRequest(settings.url + path, "POST"))
            {
                pending = request; request.uploadHandler = new UploadHandlerRaw(Encoding.UTF8.GetBytes(JsonUtility.ToJson(body)));
                request.downloadHandler = new DownloadHandlerBuffer(); request.timeout = 5;
                request.SetRequestHeader("Content-Type", "application/json"); request.SetRequestHeader("Authorization", "Bearer " + settings.adapter_token);
                yield return request.SendWebRequest();
                if (request.result != UnityWebRequest.Result.Success || request.downloadHandler.data.Length > 16384)
                    done(new Result { error = "Conversation unavailable; no reply was replayed." });
                else
                {
                    Result result;
                    try { result = JsonUtility.FromJson<Result>(request.downloadHandler.text); }
                    catch (Exception) { result = new Result { error = "Invalid conversation response." }; }
                    done(result);
                }
                if (pending == request) pending = null;
            }
        }
        private IEnumerator Turn(Sample sample, Transform speaker, int generation)
        {
            Result result = null;
            yield return Request(sample.text == null ? "/api/encounter" : "/api/turn", sample, value => result = value);
            var deadline = Time.realtimeSinceStartup + 60;
            while (generation == epoch && result != null && string.IsNullOrEmpty(result.error))
            {
                if (!Nearby(speaker) || Time.realtimeSinceStartup > deadline) { StopConversation(); yield break; }
                if (result.schema != "NAO-SOCIAL/1") break;
                if (result.state == "replied" && result.reply != null)
                {
                    bubble = result.reply.text; bubbleUntil = Time.realtimeSinceStartup + 10;
                    transcript += "\nNAO: " + bubble; status = "Your turn."; busy = false; yield break;
                }
                if (result.state != "queued" && result.state != "active") { status = result.state; busy = false; yield break; }
                status = result.state == "queued" ? "Waiting for NAO’s attention…" : "NAO is receiving your message…";
                sample.id = result.id;
                yield return new WaitForSecondsRealtime(.2f);
                if (generation != epoch) yield break;
                yield return Request("/api/poll", sample, value => result = value);
            }
            if (generation == epoch) { busy = false; status = result?.error ?? "Conversation ended."; }
        }
        public void StopConversation()
        {
            epoch++; encountered = true; busy = false; bubble = ""; pending?.Abort(); StopAllCoroutines(); status = "Conversation stopped.";
            if (settings != null && isActiveAndEnabled) StartCoroutine(Request("/api/stop", new Sample { player = principal }, _ => { }));
        }
        private void OnDisable() { epoch++; pending?.Abort(); StopAllCoroutines(); busy = false; bubble = ""; }
        private void Update()
        {
            if (!encountered && !busy && Nearby(Camera.main?.transform)) Submit(Camera.main.transform, null);
        }
        private void OnGUI()
        {
            if (settings == null) return;
            GUILayout.BeginArea(new Rect(16, Screen.height - 240, Mathf.Min(540, Screen.width - 32), 224), GUI.skin.box);
            GUILayout.Label("Talk to NAO"); GUILayout.Label(status); GUILayout.Label(transcript);
            GUI.enabled = !busy; input = GUILayout.TextField(input, 256);
            if (GUILayout.Button("Send") && Submit(Camera.main?.transform, input)) input = "";
            GUI.enabled = true; GUILayout.BeginHorizontal();
            if (GUILayout.Button("Stop conversation")) StopConversation();
            if (GUILayout.Button("Voice / accessible chat window")) Application.OpenURL(settings.url + "/#invite=" + Uri.EscapeDataString(settings.join_token));
            GUILayout.EndHorizontal(); GUILayout.EndArea();
            if (!string.IsNullOrEmpty(bubble) && Time.realtimeSinceStartup < bubbleUntil && Camera.main != null)
            {
                var point = Camera.main.WorldToScreenPoint(transform.position + Vector3.up * .8f);
                if (point.z > 0) GUI.Box(new Rect(point.x - 180, Screen.height - point.y - 60, 360, 60), "NAO: " + bubble);
            }
        }
    }
}
