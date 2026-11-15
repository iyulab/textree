using System.Diagnostics;
using System.Text.Json.Nodes;
using Microsoft.Extensions.AI;

namespace Textree.Eval;

/// <summary>
/// The time an arm takes on one case. Live, it is the wall clock. Replayed from a recording, it is
/// the wall clock of the replay plus the seconds the model took when the answers were recorded, so
/// a report made from a recording carries the time of the run it replays. Its <c>clock</c> says
/// which: <c>live</c>, <c>recorded</c>, or <c>replay_without_times</c> (a recording made before
/// times were kept — the seconds are then only the replay's).
/// </summary>
public static class Clock
{
    public readonly record struct Mark(long Started, double Replayed, int Answers, int Unrecorded);

    public static Mark Start(IChatClient chat) => chat is RecordingChatClient r
        ? new(Stopwatch.GetTimestamp(), r.ReplayedSeconds, r.Replayed, r.UnrecordedTimes)
        : new(Stopwatch.GetTimestamp(), 0, 0, 0);

    public static JsonObject Machine(IChatClient chat, Mark start)
    {
        var wall = Stopwatch.GetElapsedTime(start.Started).TotalSeconds;
        if (chat is not RecordingChatClient r || r.Replayed == start.Answers)
        {
            return new JsonObject { ["seconds"] = Math.Round(wall, 2), ["clock"] = "live" };
        }
        var seconds = wall + (r.ReplayedSeconds - start.Replayed);
        return new JsonObject
        {
            ["seconds"] = Math.Round(seconds, 2),
            ["clock"] = r.UnrecordedTimes > start.Unrecorded ? "replay_without_times" : "recorded",
        };
    }
}
