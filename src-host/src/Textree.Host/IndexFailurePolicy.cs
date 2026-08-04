namespace Textree.Host;

/// <summary>How an indexing failure should be treated at the endpoint boundary.</summary>
public enum IndexFailureKind
{
    /// <summary>The file is no longer there. Nothing left to index.</summary>
    SourceGone,

    /// <summary>The caller abandoned the request. Nobody is waiting for an answer.</summary>
    CallerGone,

    /// <summary>An actual indexing failure.</summary>
    Real,
}

/// <summary>
/// Pure decision of how to treat an exception thrown while indexing, extracted so the branch is
/// unit-testable without booting the host.
///
/// Indexing follows a live directory tree on behalf of a caller that does not read the answer, so
/// two conditions are ordinary rather than faults: the file vanished between the change
/// notification and the write, and the caller walked away (its request timed out, or the host is
/// shutting down). Reporting either as an error would fill the error channel with events nobody
/// can act on — and that channel carries no opt-out.
/// </summary>
public static class IndexFailurePolicy
{
    public static IndexFailureKind Classify(Exception ex, bool cancellationRequested)
    {
        if (ex is FileNotFoundException or DirectoryNotFoundException)
            return IndexFailureKind.SourceGone;

        // Only a cancellation the caller actually asked for is benign. A cancellation raised
        // without one is something else going wrong, and stays reportable.
        if (ex is OperationCanceledException && cancellationRequested)
            return IndexFailureKind.CallerGone;

        return IndexFailureKind.Real;
    }
}
