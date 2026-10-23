#if UNITY_EDITOR
using UnityEditor;
using UnityEditor.SceneManagement;
namespace NeuralMimicry {
public static class EmbodimentEditor {
  // Create an empty native experiment scene; runtime bootstrap owns physics.
  public static void Run() {
    EditorSceneManager.NewScene(NewSceneSetup.EmptyScene, NewSceneMode.Single);
    EditorApplication.isPlaying = true;
  }
}
}
#endif
