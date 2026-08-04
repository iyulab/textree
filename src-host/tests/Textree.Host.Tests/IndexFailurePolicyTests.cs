using Textree.Host;
using Xunit;

public class IndexFailurePolicyTests
{
    [Fact]
    public void A_missing_file_or_directory_is_a_gone_source()
    {
        Assert.Equal(IndexFailureKind.SourceGone,
            IndexFailurePolicy.Classify(new FileNotFoundException(), cancellationRequested: false));
        Assert.Equal(IndexFailureKind.SourceGone,
            IndexFailurePolicy.Classify(new DirectoryNotFoundException(), cancellationRequested: false));
    }

    [Fact]
    public void Cancellation_the_caller_asked_for_is_not_a_failure()
    {
        // The caller indexes in the background with a request timeout and drops the connection when
        // it expires — routine on a large note against a cold embedder, and not worth an error event.
        Assert.Equal(IndexFailureKind.CallerGone,
            IndexFailurePolicy.Classify(new OperationCanceledException(), cancellationRequested: true));

        // TaskCanceledException is the shape the async stack usually surfaces.
        Assert.Equal(IndexFailureKind.CallerGone,
            IndexFailurePolicy.Classify(new TaskCanceledException(), cancellationRequested: true));
    }

    [Fact]
    public void Cancellation_nobody_asked_for_stays_reportable()
    {
        // A cancellation with no request behind it means something else went wrong, so it must not
        // ride the benign path.
        Assert.Equal(IndexFailureKind.Real,
            IndexFailurePolicy.Classify(new OperationCanceledException(), cancellationRequested: false));
    }

    [Fact]
    public void Everything_else_is_a_real_failure()
    {
        Assert.Equal(IndexFailureKind.Real,
            IndexFailurePolicy.Classify(new IOException("disk"), cancellationRequested: false));
        Assert.Equal(IndexFailureKind.Real,
            IndexFailurePolicy.Classify(new InvalidOperationException(), cancellationRequested: true));
    }
}
