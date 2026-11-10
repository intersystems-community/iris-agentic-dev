Class IadSm172.Genre Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property BookId As %String;

Property GenreName As %String;

Property Popular As %Boolean;

Index PopularBitmap On Popular [ Type = bitmap ];

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Genre)</SqlIdExpression>
<SQLMap name="Master">
<Data name="BookId">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="GenreName">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Popular">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Global>^IadSm172Genre</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="PopularBitmap">
<Global>^IadSm172GenreBit</Global>
<Subscript name="1">
<Expression>{Popular}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>bitmap</Type>
</SQLMap>
<StreamLocation>^IadSm172GenreS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

