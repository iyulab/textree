using System.ClientModel;
using System.ClientModel.Primitives;
using Microsoft.Extensions.AI;
using Textree.Eval;
using Xunit;

namespace Textree.Eval.Tests;

public sealed class RecordingTests
{
    /// <summary>A model that answers with a count of the questions it was asked, failing the first <c>failures</c>.</summary>
    private sealed class Counting(int failures = 0, int status = 500) : IChatClient
    {
        public int Asked { get; private set; }

        public Task<ChatResponse> GetResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default)
        {
            Asked++;
            if (Asked <= failures)
            {
                throw new ClientResultException("server failed", new FakeResponse(status));
            }
            return Task.FromResult(new ChatResponse(new ChatMessage(ChatRole.Assistant, $"{{\"n\": {Asked}}}")));
        }

        public IAsyncEnumerable<ChatResponseUpdate> GetStreamingResponseAsync(IEnumerable<ChatMessage> messages, ChatOptions? options = null, CancellationToken cancellationToken = default) =>
            throw new NotSupportedException();

        public object? GetService(Type serviceType, object? serviceKey = null) => null;

        public void Dispose()
        {
        }
    }

    private sealed class FakeResponse(int status) : PipelineResponse
    {
        public override int Status => status;
        public override string ReasonPhrase => "";
        public override Stream? ContentStream { get; set; }
        public override BinaryData Content => BinaryData.FromString("");
        protected override PipelineResponseHeaders HeadersCore => throw new NotSupportedException();
        public override BinaryData BufferContent(CancellationToken cancellationToken = default) => Content;
        public override ValueTask<BinaryData> BufferContentAsync(CancellationToken cancellationToken = default) => new(Content);
        public override void Dispose()
        {
        }
    }

    private static List<ChatMessage> Question(string text) => [new(ChatRole.User, text)];

    [Fact]
    public async Task Recording_into_an_existing_file_continues_it_without_asking_again()
    {
        var path = Path.Combine(Path.GetTempPath(), $"recording-{Guid.NewGuid():N}.json");
        try
        {
            var first = new Counting();
            var recorder = RecordingChatClient.Record(first, path);
            var a = await recorder.GetResponseAsync(Question("one"), cancellationToken: TestContext.Current.CancellationToken);
            await recorder.GetResponseAsync(Question("two"), cancellationToken: TestContext.Current.CancellationToken);
            recorder.Save();

            var second = new Counting();
            var resumed = RecordingChatClient.Record(second, path);
            var again = await resumed.GetResponseAsync(Question("one"), cancellationToken: TestContext.Current.CancellationToken);
            await resumed.GetResponseAsync(Question("two"), cancellationToken: TestContext.Current.CancellationToken);
            await resumed.GetResponseAsync(Question("three"), cancellationToken: TestContext.Current.CancellationToken);
            resumed.Save();

            Assert.Equal(a.Text, again.Text);
            Assert.Equal(1, second.Asked);
            var replayed = RecordingChatClient.Replay(path);
            Assert.NotNull(await replayed.GetResponseAsync(Question("three"), cancellationToken: TestContext.Current.CancellationToken));
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public async Task A_passing_server_failure_is_asked_again()
    {
        var model = new Counting(failures: 1);
        var answer = await new Patient(model).GetResponseAsync(Question("one"), cancellationToken: TestContext.Current.CancellationToken);
        Assert.Equal(2, model.Asked);
        Assert.Contains("2", answer.Text);
    }

    [Fact]
    public async Task A_refused_question_is_not_asked_again()
    {
        var model = new Counting(failures: 1, status: 400);
        await Assert.ThrowsAsync<ClientResultException>(() => new Patient(model).GetResponseAsync(Question("one"), cancellationToken: TestContext.Current.CancellationToken));
        Assert.Equal(1, model.Asked);
    }
}
