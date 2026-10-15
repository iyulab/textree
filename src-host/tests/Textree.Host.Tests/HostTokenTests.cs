using System.Net;
using System.Net.Http.Headers;
using Xunit;

/// <summary>Only the app that started the host — the holder of its token — may call it.</summary>
public sealed class HostTokenTests : IClassFixture<HostFactory>
{
    private readonly HostFactory _factory;
    public HostTokenTests(HostFactory factory) => _factory = factory;

    [Theory]
    [InlineData("/health", "GET")]
    [InlineData("/search", "POST")]
    [InlineData("/index", "POST")]
    [InlineData("/chat", "POST")]
    [InlineData("/shutdown", "POST")]
    public async Task A_request_without_the_token_is_refused(string path, string method)
    {
        var client = _factory.CreateClient();
        client.DefaultRequestHeaders.Authorization = null;
        using var request = new HttpRequestMessage(new HttpMethod(method), path);
        var response = await client.SendAsync(request);
        Assert.Equal(HttpStatusCode.Unauthorized, response.StatusCode);
    }

    [Fact]
    public async Task A_request_with_another_token_is_refused()
    {
        var client = _factory.CreateClient();
        client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", new string('x', HostFactory.Token.Length));
        Assert.Equal(HttpStatusCode.Unauthorized, (await client.GetAsync("/health")).StatusCode);
    }

    [Fact]
    public async Task A_request_with_the_token_is_served()
    {
        Assert.Equal(HttpStatusCode.OK, (await _factory.CreateClient().GetAsync("/health")).StatusCode);
    }

    // A web page whose own name was pointed at 127.0.0.1 reaches the port but names itself.
    [Fact]
    public async Task A_request_naming_another_host_is_refused_even_with_the_token()
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, "/health");
        request.Headers.Host = "attacker.example";
        var response = await _factory.CreateClient().SendAsync(request);
        Assert.Equal(HttpStatusCode.BadRequest, response.StatusCode);
    }
}
