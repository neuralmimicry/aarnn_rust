using UnityEngine;

namespace NeuralMimicry
{
    // Contact callbacks arrive on the head/tail articulation objects. Forward
    // their relative speed to the one owning neural sensory frame.
    public sealed class NmCelegansContactRelay : MonoBehaviour
    {
        private NmCelegansRobot owner;
        private bool front;

        internal void Configure(NmCelegansRobot robot, bool isFront)
        {
            owner = robot;
            front = isFront;
        }

        private void OnCollisionEnter(Collision collision) => Report(collision);
        private void OnCollisionStay(Collision collision) => Report(collision);

        private void Report(Collision collision)
        {
            if (owner != null && collision != null &&
                !collision.transform.IsChildOf(owner.transform))
                owner.RecordContact(front, collision.relativeVelocity.magnitude);
        }
    }
}
